//! A client for the ZMK Studio protocol over any byte stream.

use std::io::{Read, Write};

use prost::Message;

use crate::framing::{encode, Decoder};
use crate::proto::{self, behaviors, core, keymap, meta, request, request_response, response};

#[derive(Debug, thiserror::Error)]
pub enum StudioError {
    #[error("the keyboard is locked: unlock ZMK Studio on it first")]
    Locked,
    #[error("the keyboard did not answer")]
    NoAnswer,
    #[error("the keyboard does not support this ({0})")]
    Unsupported(&'static str),
    #[error("the keyboard refused the change at key {position}: {reason}")]
    Refused {
        position: usize,
        reason: &'static str,
    },
    #[error("the keyboard could not save the changes")]
    SaveFailed,
    #[error("the keyboard sent something unexpected")]
    Protocol,
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// A behaviour the keyboard's firmware has, by the ID it uses on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceBehavior {
    pub id: u32,
    pub display_name: String,
}

pub struct Client<T> {
    stream: T,
    decoder: Decoder,
    next_id: u32,
    /// Messages already read from the stream but not yet consumed.
    pending: Vec<Vec<u8>>,
}

impl<T: Read + Write> Client<T> {
    pub fn new(stream: T) -> Self {
        Self {
            stream,
            decoder: Decoder::default(),
            next_id: 1,
            pending: Vec::new(),
        }
    }

    /// Sends a request and waits for its response, skipping notifications
    /// and responses to anything else.
    fn call(
        &mut self,
        subsystem: request::Subsystem,
    ) -> Result<request_response::Subsystem, StudioError> {
        let request_id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let request = proto::Request {
            request_id,
            subsystem: Some(subsystem),
        };
        self.stream.write_all(&encode(&request.encode_to_vec()))?;
        self.stream.flush()?;

        let mut buffer = [0u8; 256];
        loop {
            while !self.pending.is_empty() {
                let message = self.pending.remove(0);
                let response = proto::Response::decode(message.as_slice())
                    .map_err(|_| StudioError::Protocol)?;
                let Some(response::Type::RequestResponse(answer)) = response.r#type else {
                    continue;
                };
                if answer.request_id != request_id {
                    continue;
                }
                return match answer.subsystem {
                    Some(request_response::Subsystem::Meta(meta)) => {
                        Err(match meta.response_type {
                            Some(meta::response::ResponseType::SimpleError(code))
                                if code == meta::ErrorConditions::UnlockRequired as i32 =>
                            {
                                StudioError::Locked
                            }
                            Some(meta::response::ResponseType::SimpleError(code))
                                if code == meta::ErrorConditions::RpcNotFound as i32 =>
                            {
                                StudioError::Unsupported("this firmware does not have the request")
                            }
                            _ => StudioError::Protocol,
                        })
                    }
                    Some(subsystem) => Ok(subsystem),
                    None => Err(StudioError::Protocol),
                };
            }
            let read = match self.stream.read(&mut buffer) {
                Ok(0) => return Err(StudioError::NoAnswer),
                Ok(read) => read,
                Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {
                    return Err(StudioError::NoAnswer)
                }
                Err(error) => return Err(error.into()),
            };
            self.pending.extend(self.decoder.feed(&buffer[..read]));
        }
    }

    fn core(
        &mut self,
        request: core::request::RequestType,
    ) -> Result<core::response::ResponseType, StudioError> {
        let subsystem = request::Subsystem::Core(core::Request {
            request_type: Some(request),
        });
        match self.call(subsystem)? {
            request_response::Subsystem::Core(core::Response {
                response_type: Some(response),
            }) => Ok(response),
            _ => Err(StudioError::Protocol),
        }
    }

    fn keymap(
        &mut self,
        request: keymap::request::RequestType,
    ) -> Result<keymap::response::ResponseType, StudioError> {
        let subsystem = request::Subsystem::Keymap(keymap::Request {
            request_type: Some(request),
        });
        match self.call(subsystem)? {
            request_response::Subsystem::Keymap(keymap::Response {
                response_type: Some(response),
            }) => Ok(response),
            _ => Err(StudioError::Protocol),
        }
    }

    fn behaviors(
        &mut self,
        request: behaviors::request::RequestType,
    ) -> Result<behaviors::response::ResponseType, StudioError> {
        let subsystem = request::Subsystem::Behaviors(behaviors::Request {
            request_type: Some(request),
        });
        match self.call(subsystem)? {
            request_response::Subsystem::Behaviors(behaviors::Response {
                response_type: Some(response),
            }) => Ok(response),
            _ => Err(StudioError::Protocol),
        }
    }

    /// The keyboard's name. Works while locked.
    pub fn device_name(&mut self) -> Result<String, StudioError> {
        match self.core(core::request::RequestType::GetDeviceInfo(true))? {
            core::response::ResponseType::GetDeviceInfo(info) => Ok(info.name),
            _ => Err(StudioError::Protocol),
        }
    }

    /// Whether Studio is unlocked on the keyboard. Works while locked.
    pub fn is_unlocked(&mut self) -> Result<bool, StudioError> {
        match self.core(core::request::RequestType::GetLockState(true))? {
            core::response::ResponseType::GetLockState(state) => {
                Ok(state == core::LockState::Unlocked as i32)
            }
            _ => Err(StudioError::Protocol),
        }
    }

    /// Every behaviour the firmware has, with its display name.
    pub fn list_behaviors(&mut self) -> Result<Vec<DeviceBehavior>, StudioError> {
        let ids = match self.behaviors(behaviors::request::RequestType::ListAllBehaviors(true))? {
            behaviors::response::ResponseType::ListAllBehaviors(list) => list.behaviors,
            _ => return Err(StudioError::Protocol),
        };
        ids.into_iter()
            .map(|behavior_id| {
                let request = behaviors::request::RequestType::GetBehaviorDetails(
                    behaviors::GetBehaviorDetailsRequest { behavior_id },
                );
                match self.behaviors(request)? {
                    behaviors::response::ResponseType::GetBehaviorDetails(details) => {
                        Ok(DeviceBehavior {
                            id: details.id,
                            display_name: details.display_name,
                        })
                    }
                    _ => Err(StudioError::Protocol),
                }
            })
            .collect()
    }

    /// The keymap as the keyboard currently has it, unsaved changes included.
    pub fn get_keymap(&mut self) -> Result<keymap::Keymap, StudioError> {
        match self.keymap(keymap::request::RequestType::GetKeymap(true))? {
            keymap::response::ResponseType::GetKeymap(keymap) => Ok(keymap),
            _ => Err(StudioError::Protocol),
        }
    }

    /// Changes one key. The change takes effect at once but is lost at the
    /// next restart unless [`Client::save`] is called.
    pub fn set_binding(
        &mut self,
        layer_id: u32,
        position: usize,
        binding: keymap::BehaviorBinding,
    ) -> Result<(), StudioError> {
        let request =
            keymap::request::RequestType::SetLayerBinding(keymap::SetLayerBindingRequest {
                layer_id,
                key_position: position as i32,
                binding: Some(binding),
            });
        let refused = |reason| Err(StudioError::Refused { position, reason });
        match self.keymap(request)? {
            keymap::response::ResponseType::SetLayerBinding(code) => {
                match keymap::SetLayerBindingResponse::try_from(code) {
                    Ok(keymap::SetLayerBindingResponse::Ok) => Ok(()),
                    Ok(keymap::SetLayerBindingResponse::InvalidLocation) => refused("no such key"),
                    Ok(keymap::SetLayerBindingResponse::InvalidBehavior) => {
                        refused("unknown behaviour")
                    }
                    _ => refused("invalid parameters"),
                }
            }
            _ => Err(StudioError::Protocol),
        }
    }

    pub fn has_unsaved_changes(&mut self) -> Result<bool, StudioError> {
        match self.keymap(keymap::request::RequestType::CheckUnsavedChanges(true))? {
            keymap::response::ResponseType::CheckUnsavedChanges(unsaved) => Ok(unsaved),
            _ => Err(StudioError::Protocol),
        }
    }

    /// Stores the current keymap on the keyboard so it survives a restart.
    pub fn save(&mut self) -> Result<(), StudioError> {
        match self.keymap(keymap::request::RequestType::SaveChanges(true))? {
            keymap::response::ResponseType::SaveChanges(keymap::SaveChangesResponse {
                result: Some(keymap::save_changes_response::Result::Ok(_)),
            }) => Ok(()),
            keymap::response::ResponseType::SaveChanges(_) => Err(StudioError::SaveFailed),
            _ => Err(StudioError::Protocol),
        }
    }

    /// Throws away changes made since the last save.
    pub fn discard(&mut self) -> Result<(), StudioError> {
        self.keymap(keymap::request::RequestType::DiscardChanges(true))
            .map(|_| ())
    }
}
