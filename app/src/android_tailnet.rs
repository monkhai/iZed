//! One userspace Tailscale node per Android process; no VPN or root permission.
use crate::{
    android_terminal,
    ssh::TailscaleProxy,
    terminal_app::{send_native_event, NativeEvent},
};
use anyhow::{Context, Result};
use std::{
    ffi::{c_char, CStr, CString},
    sync::{mpsc, LazyLock},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

#[link(name = "tailscale")]
unsafe extern "C" {
    fn tailscale_android_interfaces(value: *const c_char) -> i32;
    fn tailscale_android_name(node: i32, value: *const c_char) -> i32;
    fn tailscale_android_prepare(node: i32) -> i32;
    fn tailscale_android_poll(node: i32);
    fn tailscale_new() -> i32;
    fn tailscale_set_dir(node: i32, path: *const c_char) -> i32;
    fn tailscale_set_hostname(node: i32, name: *const c_char) -> i32;
    fn tailscale_set_logfd(node: i32, fd: i32) -> i32;
    fn tailscale_start(node: i32) -> i32;
    fn tailscale_errmsg(node: i32, buffer: *mut c_char, length: usize) -> i32;
    fn tailscale_close(node: i32) -> i32;
    fn tailscale_status_json(node: i32, json: *mut *mut c_char) -> i32;
    fn tailscale_loopback(
        node: i32,
        address: *mut c_char,
        length: usize,
        proxy: *mut c_char,
        local: *mut c_char,
    ) -> i32;
    fn tailscale_android_login(node: i32) -> i32;
    fn tailscale_android_logout(node: i32) -> i32;
}

pub(crate) enum Command {
    Refresh,
    Login(String),
    Name(String),
    Logout,
    Stop,
}
static COMMANDS: LazyLock<mpsc::Sender<Command>> = LazyLock::new(|| {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("android-tailnet".into())
        .spawn(move || run(rx))
        .expect("Tailscale worker");
    tx
});
pub(crate) fn command(command: Command) {
    let _ = COMMANDS.send(command);
}

struct Node(i32);
impl Drop for Node {
    fn drop(&mut self) {
        unsafe {
            tailscale_close(self.0);
        }
    }
}
impl Node {
    fn new() -> Result<Self> {
        let node = Self(unsafe { tailscale_new() });
        let directory = crate::tailnet::data_dir().join("Tailscale");
        std::fs::create_dir_all(&directory)?;
        let path = CString::new(directory.to_str().context("Invalid Tailscale directory")?)?;
        let name = CString::new("ized-android")?;
        unsafe {
            check(
                tailscale_set_logfd(node.0, -1),
                "configure Tailscale logging",
            )?;
            check(
                tailscale_set_dir(node.0, path.as_ptr()),
                "configure Tailscale storage",
            )?;
            check(
                tailscale_set_hostname(node.0, name.as_ptr()),
                "configure Tailscale name",
            )?;
            check(tailscale_android_prepare(node.0), "configure Tailscale diagnostics")?;
            if tailscale_start(node.0) != 0 {
                let mut message = [0 as c_char; 1024];
                tailscale_errmsg(node.0, message.as_mut_ptr(), message.len());
                anyhow::bail!("Could not start Tailscale: {}", CStr::from_ptr(message.as_ptr()).to_string_lossy());
            }
        }
        Ok(node)
    }
    fn status(&self) -> Result<(String, serde_json::Value)> {
        let mut pointer = std::ptr::null_mut();
        unsafe {
            check(
                tailscale_status_json(self.0, &mut pointer),
                "read Tailscale status",
            )?;
            anyhow::ensure!(!pointer.is_null(), "Empty Tailscale status");
            let text = CStr::from_ptr(pointer).to_string_lossy().into_owned();
            libc::free(pointer.cast());
            let value = serde_json::from_str(&text)?;
            Ok((text, value))
        }
    }
    fn publish(&self, text: String, status: &serde_json::Value) -> Result<bool> {
        if status["BackendState"] != "Running" {
            let message = if status["BackendState"] == "NeedsLogin" {
                "Sign in to Tailscale to see your machines."
            } else {
                "Connecting to Tailscale…"
            };
            send_native_event(NativeEvent::TailscaleError(message.into()));
            return Ok(false);
        }
        let mut address = [0 as c_char; 128];
        let mut proxy = Zeroizing::new([0 as c_char; 33]);
        let mut local = Zeroizing::new([0 as c_char; 33]);
        unsafe {
            check(
                tailscale_loopback(
                    self.0,
                    address.as_mut_ptr(),
                    address.len(),
                    proxy.as_mut_ptr(),
                    local.as_mut_ptr(),
                ),
                "open Tailscale transport",
            )?;
            let endpoint: std::net::SocketAddr =
                CStr::from_ptr(address.as_ptr()).to_str()?.parse()?;
            send_native_event(NativeEvent::TailscaleUpdate(
                text,
                TailscaleProxy {
                    host: endpoint.ip().to_string(),
                    port: endpoint.port(),
                    password: Zeroizing::new(CStr::from_ptr(proxy.as_ptr()).to_str()?.to_owned()),
                },
            ));
        }
        Ok(true)
    }
}
fn check(status: i32, action: &str) -> Result<()> {
    anyhow::ensure!(
        status == 0,
        "Could not {action} ({status}). Try refreshing."
    );
    Ok(())
}
fn run(commands: mpsc::Receiver<Command>) {
    let mut node = None;
    let mut monitoring: Option<Instant> = None;
    let mut browser_opened = false;
    loop {
        let next = {
            match commands.recv_timeout(Duration::from_secs(if monitoring.is_some() { 2 } else { 30 })) {
                Ok(command) => command,
                Err(mpsc::RecvTimeoutError::Timeout) => Command::Refresh,
                Err(_) => break,
            }
        };
        if matches!(next, Command::Stop) {
            break;
        }
        let result = (|| -> Result<()> {
            let interfaces = CString::new(android_terminal::network_interfaces()?)?;
            check(unsafe { tailscale_android_interfaces(interfaces.as_ptr()) }, "read Android network interfaces")?;
            if node.is_none() {
                node = Some(Node::new()?);
            }
            let node = node.as_ref().unwrap();
            unsafe { tailscale_android_poll(node.0); }
            match next {
                Command::Login(name) => {
                    let name = CString::new(name.trim())?;
                    if !name.as_bytes().is_empty() {
                        check(unsafe { tailscale_android_name(node.0, name.as_ptr()) }, "set device name")?;
                    }
                    check(
                        unsafe { tailscale_android_login(node.0) },
                        "begin Tailscale sign-in",
                    )?;
                    monitoring = Some(Instant::now());
                    browser_opened = false;
                }
                Command::Name(name) => {
                    let name = CString::new(name.trim())?;
                    anyhow::ensure!(!name.as_bytes().is_empty(), "Enter a device name.");
                    check(unsafe { tailscale_android_name(node.0, name.as_ptr()) }, "set device name")?;
                }
                Command::Logout => {
                    monitoring = None;
                    check(
                        unsafe { tailscale_android_logout(node.0) },
                        "sign out of Tailscale",
                    )?;
                }
                _ => {}
            }
            let (text, status) = node.status()?;
            if node.publish(text, &status)? {
                monitoring = None;
            }
            if let Some(started) = monitoring {
                if started.elapsed() > Duration::from_secs(300) {
                    monitoring = None;
                    anyhow::bail!("Tailscale sign-in timed out. Try signing in again.");
                }
                if !browser_opened {
                    if let Some(url) = status["AuthURL"].as_str().filter(|url| !url.is_empty()) {
                        android_terminal::open_login(url)?;
                        browser_opened = true;
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            monitoring = None;
            send_native_event(NativeEvent::TailscaleError(error.to_string()));
        }
    }
}
