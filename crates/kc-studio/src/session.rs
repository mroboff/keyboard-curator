//! Whole operations on a connected keyboard, each opening the port, doing
//! its work and closing it again.

use kc_model::{Binding, Project};

use crate::client::{Client, StudioError};
use crate::codec::{compare, decode, BehaviorTable, Comparison};
use crate::serial;

/// A keyboard found on a serial port.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub port: String,
    pub name: String,
    pub unlocked: bool,
    /// How the project differs from the keyboard, once it is unlocked.
    pub comparison: Option<Comparison>,
}

fn open(port: &str) -> Result<Client<Box<dyn serialport::SerialPort>>, StudioError> {
    let stream =
        serial::open(port).map_err(|e| StudioError::Io(std::io::Error::other(e.to_string())))?;
    Ok(Client::new(stream))
}

/// Looks for a keyboard that answers the Studio protocol and reports how
/// `project` differs from what it is running.
pub fn find(project: &Project) -> Result<Option<Found>, StudioError> {
    for port in serial::candidate_ports() {
        let Ok(mut client) = open(&port) else {
            continue;
        };
        // Anything that does not answer is some other serial device.
        let Ok(name) = client.device_name() else {
            continue;
        };
        let unlocked = client.is_unlocked()?;
        let comparison = if unlocked {
            let table = BehaviorTable::new(project, &client.list_behaviors()?);
            Some(compare(project, &table, &client.get_keymap()?))
        } else {
            None
        };
        return Ok(Some(Found {
            port,
            name,
            unlocked,
            comparison,
        }));
    }
    Ok(None)
}

/// Applies every key that can be changed directly, saves, and returns how
/// many keys changed.
pub fn send(project: &Project, port: &str) -> Result<usize, StudioError> {
    let mut client = open(port)?;
    let table = BehaviorTable::new(project, &client.list_behaviors()?);
    let comparison = compare(project, &table, &client.get_keymap()?);
    for change in &comparison.changes {
        client.set_binding(change.layer_id, change.position, change.binding)?;
    }
    if !comparison.changes.is_empty() {
        client.save()?;
    }
    Ok(comparison.changes.len())
}

/// The keys where the keyboard differs from the project, as project
/// bindings: layer index, key position and what the keyboard has.
pub fn read(project: &Project, port: &str) -> Result<Vec<(usize, usize, Binding)>, StudioError> {
    let mut client = open(port)?;
    let table = BehaviorTable::new(project, &client.list_behaviors()?);
    let keymap = client.get_keymap()?;
    let mut differences = Vec::new();
    for layer in &keymap.layers {
        let index = layer.id as usize;
        let Some(ours) = project.layers.get(index) else {
            continue;
        };
        for (position, binding) in layer.bindings.iter().enumerate() {
            let Some(decoded) = decode(project, &table, binding) else {
                continue;
            };
            if ours.bindings.get(position).is_some_and(|b| *b != decoded) {
                differences.push((index, position, decoded));
            }
        }
    }
    Ok(differences)
}
