//! The ZMK Studio messages this client uses, written out by hand from
//! `zmk-studio-messages` at the revision ZMK v0.3.0 pins (6cb4c283), so
//! that building needs no protobuf compiler. Field numbers must match the
//! `.proto` files exactly.

#[derive(Clone, PartialEq, prost::Message)]
pub struct Request {
    #[prost(uint32, tag = "1")]
    pub request_id: u32,
    #[prost(oneof = "request::Subsystem", tags = "3, 4, 5")]
    pub subsystem: Option<request::Subsystem>,
}

pub mod request {
    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum Subsystem {
        #[prost(message, tag = "3")]
        Core(super::core::Request),
        #[prost(message, tag = "4")]
        Behaviors(super::behaviors::Request),
        #[prost(message, tag = "5")]
        Keymap(super::keymap::Request),
    }
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Response {
    #[prost(oneof = "response::Type", tags = "1, 2")]
    pub r#type: Option<response::Type>,
}

pub mod response {
    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum Type {
        #[prost(message, tag = "1")]
        RequestResponse(super::RequestResponse),
        #[prost(message, tag = "2")]
        Notification(super::Notification),
    }
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct RequestResponse {
    #[prost(uint32, tag = "1")]
    pub request_id: u32,
    #[prost(oneof = "request_response::Subsystem", tags = "2, 3, 4, 5")]
    pub subsystem: Option<request_response::Subsystem>,
}

pub mod request_response {
    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum Subsystem {
        #[prost(message, tag = "2")]
        Meta(super::meta::Response),
        #[prost(message, tag = "3")]
        Core(super::core::Response),
        #[prost(message, tag = "4")]
        Behaviors(super::behaviors::Response),
        #[prost(message, tag = "5")]
        Keymap(super::keymap::Response),
    }
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Notification {
    #[prost(oneof = "notification::Subsystem", tags = "2, 5")]
    pub subsystem: Option<notification::Subsystem>,
}

pub mod notification {
    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum Subsystem {
        #[prost(message, tag = "2")]
        Core(super::core::Notification),
        #[prost(message, tag = "5")]
        Keymap(super::keymap::Notification),
    }
}

pub mod meta {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, prost::Enumeration)]
    #[repr(i32)]
    pub enum ErrorConditions {
        Generic = 0,
        UnlockRequired = 1,
        RpcNotFound = 2,
        MsgDecodeFailed = 3,
        MsgEncodeFailed = 4,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Response {
        #[prost(oneof = "response::ResponseType", tags = "1, 2")]
        pub response_type: Option<response::ResponseType>,
    }

    pub mod response {
        #[derive(Clone, PartialEq, prost::Oneof)]
        pub enum ResponseType {
            #[prost(bool, tag = "1")]
            NoResponse(bool),
            #[prost(enumeration = "super::ErrorConditions", tag = "2")]
            SimpleError(i32),
        }
    }
}

pub mod core {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, prost::Enumeration)]
    #[repr(i32)]
    pub enum LockState {
        Locked = 0,
        Unlocked = 1,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Request {
        #[prost(oneof = "request::RequestType", tags = "1, 2, 3, 4")]
        pub request_type: Option<request::RequestType>,
    }

    pub mod request {
        #[derive(Clone, PartialEq, prost::Oneof)]
        pub enum RequestType {
            #[prost(bool, tag = "1")]
            GetDeviceInfo(bool),
            #[prost(bool, tag = "2")]
            GetLockState(bool),
            #[prost(bool, tag = "3")]
            Lock(bool),
            #[prost(bool, tag = "4")]
            ResetSettings(bool),
        }
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Response {
        #[prost(oneof = "response::ResponseType", tags = "1, 2, 4")]
        pub response_type: Option<response::ResponseType>,
    }

    pub mod response {
        #[derive(Clone, PartialEq, prost::Oneof)]
        pub enum ResponseType {
            #[prost(message, tag = "1")]
            GetDeviceInfo(super::GetDeviceInfoResponse),
            #[prost(enumeration = "super::LockState", tag = "2")]
            GetLockState(i32),
            #[prost(bool, tag = "4")]
            ResetSettings(bool),
        }
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct GetDeviceInfoResponse {
        #[prost(string, tag = "1")]
        pub name: String,
        #[prost(bytes = "vec", tag = "2")]
        pub serial_number: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Notification {
        #[prost(oneof = "notification::NotificationType", tags = "1")]
        pub notification_type: Option<notification::NotificationType>,
    }

    pub mod notification {
        #[derive(Clone, PartialEq, prost::Oneof)]
        pub enum NotificationType {
            #[prost(enumeration = "super::LockState", tag = "1")]
            LockStateChanged(i32),
        }
    }
}

pub mod behaviors {
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Request {
        #[prost(oneof = "request::RequestType", tags = "1, 2")]
        pub request_type: Option<request::RequestType>,
    }

    pub mod request {
        #[derive(Clone, PartialEq, prost::Oneof)]
        pub enum RequestType {
            #[prost(bool, tag = "1")]
            ListAllBehaviors(bool),
            #[prost(message, tag = "2")]
            GetBehaviorDetails(super::GetBehaviorDetailsRequest),
        }
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct GetBehaviorDetailsRequest {
        #[prost(uint32, tag = "1")]
        pub behavior_id: u32,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Response {
        #[prost(oneof = "response::ResponseType", tags = "1, 2")]
        pub response_type: Option<response::ResponseType>,
    }

    pub mod response {
        #[derive(Clone, PartialEq, prost::Oneof)]
        pub enum ResponseType {
            #[prost(message, tag = "1")]
            ListAllBehaviors(super::ListAllBehaviorsResponse),
            #[prost(message, tag = "2")]
            GetBehaviorDetails(super::GetBehaviorDetailsResponse),
        }
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct ListAllBehaviorsResponse {
        #[prost(uint32, repeated, tag = "1")]
        pub behaviors: Vec<u32>,
    }

    /// The parameter metadata (field 3) is not read; the client knows
    /// each behavior's parameters from its own catalog.
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct GetBehaviorDetailsResponse {
        #[prost(uint32, tag = "1")]
        pub id: u32,
        #[prost(string, tag = "2")]
        pub display_name: String,
    }
}

pub mod keymap {
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Request {
        #[prost(oneof = "request::RequestType", tags = "1, 2, 3, 4, 5")]
        pub request_type: Option<request::RequestType>,
    }

    pub mod request {
        #[derive(Clone, PartialEq, prost::Oneof)]
        pub enum RequestType {
            #[prost(bool, tag = "1")]
            GetKeymap(bool),
            #[prost(message, tag = "2")]
            SetLayerBinding(super::SetLayerBindingRequest),
            #[prost(bool, tag = "3")]
            CheckUnsavedChanges(bool),
            #[prost(bool, tag = "4")]
            SaveChanges(bool),
            #[prost(bool, tag = "5")]
            DiscardChanges(bool),
        }
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Response {
        #[prost(oneof = "response::ResponseType", tags = "1, 2, 3, 4, 5")]
        pub response_type: Option<response::ResponseType>,
    }

    pub mod response {
        #[derive(Clone, PartialEq, prost::Oneof)]
        pub enum ResponseType {
            #[prost(message, tag = "1")]
            GetKeymap(super::Keymap),
            #[prost(enumeration = "super::SetLayerBindingResponse", tag = "2")]
            SetLayerBinding(i32),
            #[prost(bool, tag = "3")]
            CheckUnsavedChanges(bool),
            #[prost(message, tag = "4")]
            SaveChanges(super::SaveChangesResponse),
            #[prost(bool, tag = "5")]
            DiscardChanges(bool),
        }
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Notification {
        #[prost(oneof = "notification::NotificationType", tags = "1")]
        pub notification_type: Option<notification::NotificationType>,
    }

    pub mod notification {
        #[derive(Clone, PartialEq, prost::Oneof)]
        pub enum NotificationType {
            #[prost(bool, tag = "1")]
            UnsavedChangesStatusChanged(bool),
        }
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct SaveChangesResponse {
        #[prost(oneof = "save_changes_response::Result", tags = "1, 2")]
        pub result: Option<save_changes_response::Result>,
    }

    pub mod save_changes_response {
        #[derive(Clone, PartialEq, prost::Oneof)]
        pub enum Result {
            #[prost(bool, tag = "1")]
            Ok(bool),
            #[prost(int32, tag = "2")]
            Err(i32),
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, prost::Enumeration)]
    #[repr(i32)]
    pub enum SetLayerBindingResponse {
        Ok = 0,
        InvalidLocation = 1,
        InvalidBehavior = 2,
        InvalidParameters = 3,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct SetLayerBindingRequest {
        #[prost(uint32, tag = "1")]
        pub layer_id: u32,
        #[prost(int32, tag = "2")]
        pub key_position: i32,
        #[prost(message, optional, tag = "3")]
        pub binding: Option<BehaviorBinding>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Keymap {
        #[prost(message, repeated, tag = "1")]
        pub layers: Vec<Layer>,
        #[prost(uint32, tag = "2")]
        pub available_layers: u32,
        #[prost(uint32, tag = "3")]
        pub max_layer_name_length: u32,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Layer {
        #[prost(uint32, tag = "1")]
        pub id: u32,
        #[prost(string, tag = "2")]
        pub name: String,
        #[prost(message, repeated, tag = "3")]
        pub bindings: Vec<BehaviorBinding>,
    }

    #[derive(Clone, Copy, PartialEq, Eq, Hash, prost::Message)]
    pub struct BehaviorBinding {
        #[prost(sint32, tag = "1")]
        pub behavior_id: i32,
        #[prost(uint32, tag = "2")]
        pub param1: u32,
        #[prost(uint32, tag = "3")]
        pub param2: u32,
    }
}
