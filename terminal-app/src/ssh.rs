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

pub async fn probe(host: Host) -> Result<String> {
    smol::unblock(move || {
        let (_, hostname, port) = host.target()?;
        let identity = Arc::new(Mutex::new(None));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let result = runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(12), async {
                client::connect(
                    Arc::new(client::Config::default()),
                    (hostname.as_str(), port),
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

/// Refresh discovery through a host the user has already paired with this phone.
pub async fn tailnet_status(host: Host, key: Zeroizing<String>) -> Result<String> {
    smol::unblock(move || {
        let (user, hostname, port) = host.target()?;
        let fingerprint = host.fingerprint.context("Connect to the roster source once to verify its SSH identity")?;
        let private_key = russh::keys::PrivateKey::from_openssh(key.as_str())?;
        drop(key);
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(20), async {
                let mut handle = client::connect(Arc::new(client::Config::default()), (hostname.as_str(), port), Verify(fingerprint)).await?;
                anyhow::ensure!(handle.authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(private_key), None)).await?.success(), "Authorize this phone’s SSH key on the roster source first");
                let mut channel = handle.channel_open_session().await?;
                channel.exec(true, "if [ -x /Applications/Tailscale.app/Contents/MacOS/Tailscale ]; then /Applications/Tailscale.app/Contents/MacOS/Tailscale status --json; else tailscale status --json; fi").await?;
                let mut bytes = Vec::new();
                while let Some(message) = channel.wait().await {
                    match message {
                        ChannelMsg::Data { data } => { anyhow::ensure!(bytes.len() + data.len() <= 2 * 1024 * 1024, "The device roster is too large"); bytes.extend_from_slice(&data); }
                        ChannelMsg::ExitStatus { exit_status } if exit_status != 0 => bail!("Tailscale could not read the device roster on this host"),
                        _ => {}
                    }
                }
                Ok(String::from_utf8(bytes)?)
            }).await.context("Roster refresh timed out")?
        })
    }).await
}

pub async fn open(host: Host, key: Zeroizing<String>) -> Result<Session> {
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
                let config = Arc::new(client::Config {
                    keepalive_interval: Some(Duration::from_secs(15)),
                    keepalive_max: 3,
                    ..Default::default()
                });
                let mut handle = tokio::time::timeout(Duration::from_secs(15), client::connect(config, (hostname.as_str(), port), Verify(fingerprint))).await.context("SSH connection timed out")??;
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
