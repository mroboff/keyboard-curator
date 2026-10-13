//! Whole operations on a connected keyboard, each opening the port, doing
//! its work and closing it again.

use kc_boards::geometry::Key;
use kc_boards::Board;
use kc_model::{Binding, Device, Project};

use crate::client::{Client, StudioError};
use crate::codec::{compare, decode, BehaviorTable, Comparison};
use crate::serial;

/// A keyboard found on a serial port.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub port: String,
    /// The USB device behind the port, when it reports a serial number.
    pub device: Option<Device>,
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
        let Ok(mut client) = open(&port.name) else {
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
            port: port.name,
            device: port.device,
            name,
            unlocked,
            comparison,
        }));
    }
    Ok(None)
}

/// A keyboard found on a serial port, with what it says about itself:
/// enough to make a board for one the app has no file for.
#[derive(Debug, Clone, PartialEq)]
pub struct Discovered {
    pub port: String,
    pub device: Option<Device>,
    pub name: String,
    pub unlocked: bool,
    /// Its physical layouts, by name, with the keys in binding order.
    /// Empty while the keyboard is locked, which keeps them from it.
    pub layouts: Vec<(String, Vec<Key>)>,
    /// Which of them it is using.
    pub active: usize,
}

/// Every keyboard on a serial port that answers the Studio protocol.
pub fn discover() -> Vec<Discovered> {
    let mut found = Vec::new();
    for port in serial::candidate_ports() {
        let Ok(mut client) = open(&port.name) else {
            continue;
        };
        let Ok(name) = client.device_name() else {
            continue;
        };
        let unlocked = client.is_unlocked().unwrap_or(false);
        let (layouts, active) = match unlocked.then(|| client.physical_layouts()) {
            Some(Ok(reported)) => (
                reported
                    .layouts
                    .iter()
                    .map(|layout| {
                        let keys = layout
                            .keys
                            .iter()
                            .map(|k| Key::from([k.width, k.height, k.x, k.y, k.r, k.rx, k.ry]))
                            .collect();
                        (layout.name.clone(), keys)
                    })
                    .collect(),
                reported.active_layout_index as usize,
            ),
            _ => (Vec::new(), 0),
        };
        found.push(Discovered {
            port: port.name,
            device: port.device,
            name,
            unlocked,
            layouts,
            active,
        });
    }
    found
}

/// Reads the whole keymap of the first keyboard that answers, as a new
/// layout for `board`: every layer by name, every key the app can name.
/// Keys bound to behaviors the app does not know are left transparent.
pub fn read_board(board: &Board, name: &str) -> Result<Project, StudioError> {
    for port in serial::candidate_ports() {
        let Ok(mut client) = open(&port.name) else {
            continue;
        };
        if client.device_name().is_err() {
            continue;
        }
        if !client.is_unlocked()? {
            return Err(StudioError::Locked);
        }
        let mut project = Project::new(name, board);
        let table = BehaviorTable::new(&project, &client.list_behaviors()?);
        let keymap = client.get_keymap()?;
        for (index, layer) in keymap.layers.iter().enumerate() {
            let id = if index == 0 {
                project.layers[0].id
            } else {
                project
                    .add_layer(layer.name.clone())
                    .map_err(|_| StudioError::Unsupported("more layers than a layout can hold"))?
            };
            if !layer.name.trim().is_empty() {
                if let Ok(ours) = project.layer_mut(id) {
                    ours.name = layer.name.clone();
                }
            }
            let decoded: Vec<(usize, Binding)> = layer
                .bindings
                .iter()
                .enumerate()
                .filter_map(|(position, binding)| {
                    decode(&project, &table, binding).map(|b| (position, b))
                })
                .collect();
            for (position, binding) in decoded {
                let _ = project.set_binding(id, position, binding);
            }
        }
        return Ok(project);
    }
    Err(StudioError::NotFound)
}

/// Sends a layout to the keyboard it is for, and returns how many keys
/// changed.
pub fn send_board(project: &Project) -> Result<usize, StudioError> {
    let found = find(project)?.ok_or(StudioError::NotFound)?;
    if !found.unlocked {
        return Err(StudioError::Locked);
    }
    send(project, &found.port)
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
