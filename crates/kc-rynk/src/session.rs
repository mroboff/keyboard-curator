//! One conversation with a keyboard: find it, connect, do the work while
//! the protocol driver pumps bytes, and close.

use std::time::Duration;

use embassy_futures::select::{select, Either};
use kc_boards::Board;
use kc_model::Device as Link;
use rynk::{Client, RynkDevice, RynkHostError};

use crate::transport;
use crate::RynkError;

/// How long the handshake may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// What the keyboard said about itself on connecting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// The USB product name.
    pub label: String,
    /// The USB serial number, without Rynk's own mark.
    pub serial: Option<String>,
    pub protocol: (u8, u8),
    /// The firmware's own description of its build.
    pub build: String,
    pub rows: u8,
    pub cols: u8,
    pub layers: u8,
    /// Whether the keyboard lets a host change its configuration now.
    pub unlocked: bool,
}

pub(crate) fn host_error(error: RynkHostError) -> RynkError {
    match error {
        RynkHostError::Disconnected => RynkError::Disconnected,
        RynkHostError::VersionMismatch { .. } => RynkError::Protocol(error.to_string()),
        RynkHostError::Transport(_, _) | RynkHostError::Io(_) => {
            RynkError::Transport(error.to_string())
        }
        other => RynkError::Protocol(other.to_string()),
    }
}

/// Finds the keyboard, connects, and runs `work` against it. The driver
/// that moves bytes runs alongside; when the link dies, `work` is
/// dropped where it stands.
pub(crate) fn run<T>(
    board: &Board,
    link: Option<&Link>,
    budget: Duration,
    work: impl AsyncFnOnce(&Client, &Identity) -> Result<T, RynkError>,
) -> Result<T, RynkError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| RynkError::Transport(e.to_string()))?;
    runtime.block_on(async {
        let devices = transport::discover(board, link).await.map_err(host_error)?;
        let device = devices
            .into_iter()
            .next()
            .ok_or_else(|| RynkError::NotFound(board.name.clone()))?;
        let (label, serial) = (device.label(), device.serial.clone());
        let (client, mut driver) = tokio::time::timeout(CONNECT_TIMEOUT, device.connect())
            .await
            .map_err(|_| RynkError::Timeout("waiting for the keyboard to answer"))?
            .map_err(host_error)?;
        let session = async {
            let identity = identify(&client, label, serial).await?;
            work(&client, &identity).await
        };
        match tokio::time::timeout(budget, select(driver.run(&client), session)).await {
            Ok(Either::First(error)) => Err(host_error(error)),
            Ok(Either::Second(result)) => result,
            Err(_) => Err(RynkError::Timeout("talking to the keyboard")),
        }
    })
}

async fn identify(
    client: &Client,
    label: String,
    serial: Option<String>,
) -> Result<Identity, RynkError> {
    let version = client.get_version().await.map_err(host_error)?;
    let info = client.get_device_info().await.map_err(host_error)?;
    let build = client
        .get_build_info()
        .await
        .map(|b| b.label.as_str().to_string())
        .unwrap_or_default();
    let capabilities = client.get_capabilities().await.map_err(host_error)?;
    let unlocked = client
        .get_maintenance_mode()
        .await
        .map(|m| m.enabled)
        .unwrap_or(true);
    let label = if label.is_empty() {
        info.product_name.as_str().to_string()
    } else {
        label
    };
    let serial = serial
        .as_deref()
        .map(transport::bare_serial)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            let serial = transport::bare_serial(info.serial_number.as_str());
            (!serial.is_empty()).then(|| serial.to_string())
        });
    Ok(Identity {
        label,
        serial,
        protocol: (version.major, version.minor),
        build,
        rows: capabilities.num_rows,
        cols: capabilities.num_cols,
        layers: capabilities.num_layers,
        unlocked,
    })
}
