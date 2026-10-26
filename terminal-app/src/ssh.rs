//! Direct SSH PTYs, adapted from iZed's iOS transport. No Zed server or RPC.
use anyhow::{bail, Context, Result};
use russh::{
    client,
    keys::{HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate},
    ChannelMsg,
};
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use terminal::RemoteTerminalChannels;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use zeroize::Zeroizing;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Host {
    pub name: String,
    pub address: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub fingerprint: Option<String>,
}

impl Host {
    pub fn target(&self) -> Result<(String, String, u16)> {
        let address = self
            .address
            .trim()
            .strip_prefix("ssh://")
            .unwrap_or(self.address.trim());
        let (user, endpoint) = address
            .split_once('@')
            .context("Use user@hostname or user@hostname:port")?;
        anyhow::ensure!(
            !user.is_empty() && !endpoint.is_empty(),
            "Enter both a user and a host"
        );
        let (host, port) = if endpoint.starts_with('[') {
            let (host, tail) = endpoint[1..]
                .split_once(']')
                .context("Close the IPv6 address with ]")?;
            (
                host.to_owned(),
                if tail.is_empty() {
                    22
                } else {
                    tail.strip_prefix(':').context("Invalid port")?.parse()?
                },
            )
        } else if let Some((host, port)) = endpoint.rsplit_once(':') {
            anyhow::ensure!(!host.contains(':'), "Wrap IPv6 addresses in brackets");
            (host.to_owned(), port.parse()?)
        } else {
            (endpoint.to_owned(), 22)
        };
        anyhow::ensure!(
            !host.is_empty() && port != 0 && !host.chars().any(char::is_whitespace),
            "Invalid host or port"
        );
        Ok((user.to_owned(), host, port))
    }
}

#[derive(Clone)]
pub struct TailscaleProxy {
    pub host: String,
    pub port: u16,
    pub password: Zeroizing<String>,
}

async fn tailscale_stream(proxy: &TailscaleProxy, host: &str, port: u16) -> Result<TcpStream> {
    let mut stream = TcpStream::connect((proxy.host.as_str(), proxy.port)).await?;

    // TailscaleKit exposes a loopback SOCKS5 proxy for outbound tailnet TCP.
    stream.write_all(&[5, 1, 2]).await?;
    let mut method = [0; 2];
    stream.read_exact(&mut method).await?;
    anyhow::ensure!(method == [5, 2], "Tailscale proxy rejected authentication");

    let username = b"tsnet";
    let password = proxy.password.as_bytes();
    anyhow::ensure!(
        password.len() <= u8::MAX as usize,
        "Invalid Tailscale proxy credential"
    );
    let mut auth = Vec::with_capacity(3 + password.len());
    auth.extend_from_slice(&[1, username.len() as u8]);
    auth.extend_from_slice(username);
    auth.push(password.len() as u8);
    auth.extend_from_slice(password);
    stream.write_all(&auth).await?;
    auth.fill(0);
    let mut auth_result = [0; 2];
    stream.read_exact(&mut auth_result).await?;
    anyhow::ensure!(
        auth_result == [1, 0],
        "Tailscale proxy authentication failed"
    );

    let mut request = vec![5, 1, 0];
    if let Ok(address) = host.parse::<std::net::Ipv4Addr>() {
        request.push(1);
        request.extend_from_slice(&address.octets());
    } else if let Ok(address) = host.parse::<std::net::Ipv6Addr>() {
        request.push(4);
        request.extend_from_slice(&address.octets());
    } else {
        anyhow::ensure!(
            host.len() <= u8::MAX as usize,
            "Tailscale host name is too long"
        );
        request.extend_from_slice(&[3, host.len() as u8]);
        request.extend_from_slice(host.as_bytes());
    }
    request.extend_from_slice(&port.to_be_bytes());
    stream.write_all(&request).await?;

    let mut reply = [0; 4];
    stream.read_exact(&mut reply).await?;
    anyhow::ensure!(
        reply[0] == 5 && reply[1] == 0,
        "Could not reach this machine over Tailscale"
    );
    match reply[3] {
        1 => {
            let mut address_and_port = [0; 6];
            stream.read_exact(&mut address_and_port).await?;
        }
        3 => {
            let mut length = [0; 1];
            stream.read_exact(&mut length).await?;
            let mut address_and_port = vec![0; length[0] as usize + 2];
            stream.read_exact(&mut address_and_port).await?;
        }
        4 => {
            let mut address_and_port = [0; 18];
            stream.read_exact(&mut address_and_port).await?;
        }
        _ => anyhow::bail!("Tailscale returned an invalid proxy response"),
    }
    Ok(stream)
}

struct Probe(Arc<Mutex<Option<String>>>);
impl client::Handler for Probe {
    type Error = russh::Error;
    async fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        *self.0.lock().unwrap() = Some(key.public_key().fingerprint(HashAlg::Sha256).to_string());
        Ok(false)
    }
}

pub async fn probe(host: Host, proxy: TailscaleProxy) -> Result<String> {
    smol::unblock(move || {
        let (_, hostname, port) = host.target()?;
        let identity = Arc::new(Mutex::new(None));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let result = runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(20), async {
                let stream = tailscale_stream(&proxy, &hostname, port)
                    .await
                    .map_err(std::io::Error::other)?;
                client::connect_stream(
                    Arc::new(client::Config::default()),
                    stream,
                    Probe(identity.clone()),
                )
                .await
            })
            .await
        });
        let fingerprint = identity
            .lock()
            .unwrap()
            .clone()
            .with_context(|| match result {
                Err(_) => "SSH identity check timed out. Check the address and network connection."
                    .to_owned(),
                Ok(Err(error)) => format!("Could not read the host identity: {error}"),
                Ok(Ok(_)) => "The SSH host did not present a supported identity.".to_owned(),
            })?;
        Ok(fingerprint)
    })
    .await
}

struct Verify(String);
impl client::Handler for Verify {
    type Error = russh::Error;
    async fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        Ok(key.public_key().fingerprint(HashAlg::Sha256).to_string() == self.0)
    }
}

pub struct Session {
    pub channels: RemoteTerminalChannels,
    pub ended: smol::channel::Receiver<String>,
}

pub async fn open(host: Host, key: Zeroizing<String>, proxy: TailscaleProxy) -> Result<Session> {
    let (input_tx, input_rx) = smol::channel::bounded::<Vec<u8>>(256);
    let (resize_tx, resize_rx) = smol::channel::unbounded();
    let (output_tx, output_rx) = smol::channel::bounded::<Vec<u8>>(256);
    let (cancel_tx, cancel_rx) = smol::channel::bounded::<()>(1);
    let (ready_tx, ready_rx) = smol::channel::bounded(1);
    let (ended_tx, ended_rx) = smol::channel::bounded(1);
    std::thread::Builder::new().name("ssh-terminal".into()).spawn(move || {
        let result = (|| -> Result<()> {
            let (user, hostname, port) = host.target()?;
            let fingerprint = host.fingerprint.context("Verify the host identity before connecting")?;
            let private_key = russh::keys::PrivateKey::from_openssh(key.as_str())?;
            drop(key);
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
            runtime.block_on(async {
                let config = Arc::new(client::Config::default());
                let mut handle = tokio::time::timeout(Duration::from_secs(20), async {
                    let stream = tailscale_stream(&proxy, &hostname, port)
                        .await
                        .map_err(std::io::Error::other)?;
                    client::connect_stream(config, stream, Verify(fingerprint)).await
                })
                .await
                .context("SSH connection timed out")??;
                let auth = tokio::time::timeout(Duration::from_secs(15), handle.authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(private_key), None))).await.context("SSH authentication timed out")??;
                if !auth.success() { bail!("Key not authorized. Add this phone’s public key to ~/.ssh/authorized_keys on your host."); }
                let mut channel = handle.channel_open_session().await?;
                channel.request_pty(true, "xterm-256color", 40, 24, 0, 0, &[]).await?;
                if host.command.trim().is_empty() { channel.request_shell(true).await?; }
                else { channel.exec(true, host.command.as_str()).await?; }
                ready_tx.send(Ok(())).await.ok();
                loop {
                    tokio::select! {
                        _ = cancel_rx.recv() => { channel.close().await.ok(); break; }
                        message = channel.wait() => match message {
                            Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                                tokio::select! {
                                    _ = cancel_rx.recv() => break,
                                    sent = output_tx.send(data.to_vec()) => { if sent.is_err() { break; } }
                                }
                            }
                            Some(ChannelMsg::ExitStatus { exit_status }) => { return Ok(format!("Shell exited ({exit_status})")); }
                            Some(ChannelMsg::Eof | ChannelMsg::Close) | None => break,
                            _ => {}
                        },
                        input = input_rx.recv() => match input {
                            Ok(bytes) => channel.data_bytes(bytes).await?,
                            Err(_) => break,
                        },
                        resize = resize_rx.recv() => if let Ok((columns, rows)) = resize {
                            channel.window_change(columns, rows, 0, 0).await?;
                        },
                    }
                }
                Ok("Disconnected".to_owned())
            }).map(|message| { ended_tx.try_send(message).ok(); })
        })();
        if let Err(error) = result {
            let message = format!("{error:#}");
            ready_tx.try_send(Err(message.clone())).ok();
            ended_tx.try_send(message).ok();
        }
    })?;
    ready_rx
        .recv()
        .await
        .context("SSH connection stopped")?
        .map_err(anyhow::Error::msg)?;
    Ok(Session {
        channels: RemoteTerminalChannels {
            input_tx,
            resize_tx,
            output_rx,
            cancel_tx: Some(cancel_tx),
        },
        ended: ended_rx,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn target(address: &str) -> Result<(String, String, u16)> {
        Host {
            name: "test".into(),
            address: address.into(),
            command: String::new(),
            fingerprint: None,
        }
        .target()
    }
    #[test]
    fn parses_ssh_targets() {
        assert_eq!(target("me@host").unwrap(), ("me".into(), "host".into(), 22));
        assert_eq!(target("ssh://me@host:2222").unwrap().2, 2222);
        assert_eq!(target("me@[::1]:2222").unwrap().1, "::1");
        for invalid in ["host", "@host", "me@", "me@host:0", "me@::1", "me@bad host"] {
            assert!(target(invalid).is_err(), "{invalid}");
        }
    }
}
