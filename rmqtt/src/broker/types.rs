use std::any::Any;
use std::convert::From as _f;
use std::fmt;
use std::fmt::Display;
use std::net::SocketAddr;
use std::num::{NonZeroU16, NonZeroU32};
use std::ops::Deref;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bitflags::bitflags;
use bytestring::ByteString;
use itertools::Itertools;
use ntex::util::Bytes;
use ntex_mqtt::error::SendPacketError;
pub use ntex_mqtt::types::{Protocol, MQTT_LEVEL_31, MQTT_LEVEL_311, MQTT_LEVEL_5};
pub use ntex_mqtt::v3::{
    self, codec::Connect as ConnectV3, codec::ConnectAckReason as ConnectAckReasonV3,
    codec::LastWill as LastWillV3, codec::Packet as PacketV3,
    codec::SubscribeReturnCode as SubscribeReturnCodeV3, HandshakeAck as HandshakeAckV3,
    MqttSink as MqttSinkV3,
};
pub use ntex_mqtt::v5::{
    self, codec::Connect as ConnectV5, codec::ConnectAckReason as ConnectAckReasonV5,
    codec::Disconnect as DisconnectV5, codec::DisconnectReasonCode, codec::LastWill as LastWillV5,
    codec::Packet as PacketV5, codec::PublishAck2, codec::PublishAck2Reason,
    codec::PublishProperties as PublishPropertiesV5, codec::Subscribe as SubscribeV5,
    codec::SubscribeAck as SubscribeAckV5, codec::SubscribeAckReason, codec::SubscriptionOptions,
    codec::Unsubscribe as UnsubscribeV5, codec::UnsubscribeAck as UnsubscribeAckV5, codec::UserProperties,
    codec::UserProperty, HandshakeAck as HandshakeAckV5, MqttSink as MqttSinkV5,
};
use serde::de::{Deserialize, Deserializer};
use serde::ser::{Serialize, SerializeStruct, Serializer};
use tokio::sync::oneshot;

use crate::{MqttError, Result, Runtime};

pub type NodeId = u64;
pub type NodeName = String;
pub type RemoteSocketAddr = SocketAddr;
pub type LocalSocketAddr = SocketAddr;
pub type Addr = bytestring::ByteString;
pub type ClientId = bytestring::ByteString;
pub type UserName = bytestring::ByteString;
pub type Superuser = bool;
pub type Password = bytes::Bytes;
pub type PacketId = u16;
///topic name or topic filter
pub type TopicName = bytestring::ByteString;
pub type Topic = ntex_mqtt::Topic;
///topic filter
pub type TopicFilter = bytestring::ByteString;
pub type SharedGroup = String;
pub type IsDisconnect = bool;
pub type MessageExpiry = bool;
pub type TimestampMillis = i64;
pub type Timestamp = i64;
pub type IsOnline = bool;
pub type IsAdmin = bool;
pub type LimiterName = u16;
pub type CleanStart = bool;

pub type Tx = futures::channel::mpsc::UnboundedSender<Message>;
pub type Rx = futures::channel::mpsc::UnboundedReceiver<Message>;

pub type DashSet<V> = dashmap::DashSet<V, ahash::RandomState>;
pub type DashMap<K, V> = dashmap::DashMap<K, V, ahash::RandomState>;
pub type HashMap<K, V> = std::collections::HashMap<K, V, ahash::RandomState>;
pub type QoS = ntex_mqtt::types::QoS;
pub type PublishReceiveTime = TimestampMillis;
pub type Subscriptions = Vec<(TopicFilter, SubscriptionValue)>;
pub type TopicFilters = Vec<TopicFilter>;
pub type SubscriptionValue = (QoS, Option<SharedGroup>);
pub type SubscriptionSize = usize;

pub type HookSubscribeResult = Vec<Option<TopicFilter>>;
pub type HookUnsubscribeResult = Vec<Option<TopicFilter>>;

pub(crate) const UNDEFINED: &str = "undefined";

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ConnectInfo {
    V3(Id, ConnectV3),
    V5(Id, Box<ConnectV5>),
}

impl ConnectInfo {
    #[inline]
    pub fn id(&self) -> &Id {
        match self {
            ConnectInfo::V3(id, _) => id,
            ConnectInfo::V5(id, _) => id,
        }
    }

    #[inline]
    pub fn client_id(&self) -> &ClientId {
        match self {
            ConnectInfo::V3(_, c) => &c.client_id,
            ConnectInfo::V5(_, c) => &c.client_id,
        }
    }

    #[inline]
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            ConnectInfo::V3(id, conn_info) => {
                json!({
                    "node": id.node(),
                    "ipaddress": id.remote_addr,
                    "clientid": id.client_id,
                    "username": id.username_ref(),
                    "keepalive": conn_info.keep_alive,
                    "proto_ver": conn_info.protocol.level(),
                    "clean_session": conn_info.clean_session,
                    "last_will": self.last_will().map(|lw|lw.to_json())
                })
            }
            ConnectInfo::V5(id, conn_info) => {
                json!({
                    "node": id.node(),
                    "ipaddress": id.remote_addr,
                    "clientid": id.client_id,
                    "username": id.username_ref(),
                    "keepalive": conn_info.keep_alive,
                    "proto_ver": ntex_mqtt::types::MQTT_LEVEL_5,
                    "clean_start": conn_info.clean_start,
                    "last_will": self.last_will().map(|lw|lw.to_json()),

                    "session_expiry_interval_secs": conn_info.session_expiry_interval_secs,
                    "auth_method": conn_info.auth_method,
                    "auth_data": conn_info.auth_data,
                    "request_problem_info": conn_info.request_problem_info,
                    "request_response_info": conn_info.request_response_info,
                    "receive_max": conn_info.receive_max,
                    "topic_alias_max": conn_info.topic_alias_max,
                    "user_properties": conn_info.user_properties,
                    "max_packet_size": conn_info.max_packet_size,
                })
            }
        }
    }

    #[inline]
    pub fn to_hook_body(&self) -> serde_json::Value {
        match self {
            ConnectInfo::V3(id, conn_info) => {
                json!({
                    "node": id.node(),
                    "ipaddress": id.remote_addr,
                    "clientid": id.client_id,
                    "username": id.username_ref(),
                    "keepalive": conn_info.keep_alive,
                    "proto_ver": conn_info.protocol.level(),
                    "clean_session": conn_info.clean_session,
                })
            }
            ConnectInfo::V5(id, conn_info) => {
                json!({
                    "node": id.node(),
                    "ipaddress": id.remote_addr,
                    "clientid": id.client_id,
                    "username": id.username_ref(),
                    "keepalive": conn_info.keep_alive,
                    "proto_ver": ntex_mqtt::types::MQTT_LEVEL_5,
                    "clean_start": conn_info.clean_start,
                })
            }
        }
    }

    #[inline]
    pub fn last_will(&self) -> Option<LastWill> {
        match self {
            ConnectInfo::V3(_, conn_info) => conn_info.last_will.as_ref().map(LastWill::V3),
            ConnectInfo::V5(_, conn_info) => conn_info.last_will.as_ref().map(LastWill::V5),
        }
    }

    #[inline]
    pub fn keep_alive(&self) -> u16 {
        match self {
            ConnectInfo::V3(_, conn_info) => conn_info.keep_alive,
            ConnectInfo::V5(_, conn_info) => conn_info.keep_alive,
        }
    }

    #[inline]
    pub fn username(&self) -> Option<&UserName> {
        match self {
            ConnectInfo::V3(_, conn_info) => conn_info.username.as_ref(),
            ConnectInfo::V5(_, conn_info) => conn_info.username.as_ref(),
        }
    }

    #[inline]
    pub fn password(&self) -> Option<&Password> {
        match self {
            ConnectInfo::V3(_, conn_info) => conn_info.password.as_ref(),
            ConnectInfo::V5(_, conn_info) => conn_info.password.as_ref(),
        }
    }

    #[inline]
    pub fn clean_start(&self) -> bool {
        match self {
            ConnectInfo::V3(_, conn_info) => conn_info.clean_session,
            ConnectInfo::V5(_, conn_info) => conn_info.clean_start,
        }
    }

    #[inline]
    pub fn proto_ver(&self) -> u8 {
        match self {
            ConnectInfo::V3(_, conn_info) => conn_info.protocol.level(),
            ConnectInfo::V5(_, _) => MQTT_LEVEL_5,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disconnect {
    V3,
    V5(DisconnectV5),
}

impl Disconnect {
    #[inline]
    pub fn reason_code(&self) -> Option<DisconnectReasonCode> {
        match self {
            Disconnect::V3 => None,
            Disconnect::V5(d) => Some(d.reason_code),
        }
    }

    #[inline]
    pub fn reason(&self) -> Reason {
        match self {
            Disconnect::V3 => Reason::ConnectDisconnect(None),
            Disconnect::V5(d) => Reason::ConnectDisconnect(d.reason_string.as_ref().cloned()),
        }
    }
}

pub trait QoSEx {
    fn value(&self) -> u8;
    fn less_value(&self, qos: QoS) -> QoS;
}

impl QoSEx for QoS {
    #[inline]
    fn value(&self) -> u8 {
        match self {
            QoS::AtMostOnce => 0,
            QoS::AtLeastOnce => 1,
            QoS::ExactlyOnce => 2,
        }
    }

    #[inline]
    fn less_value(&self, qos: QoS) -> QoS {
        if self.value() < qos.value() {
            *self
        } else {
            qos
        }
    }
}

pub type SubscribeAclResult = SubscribeReturn;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishAclResult {
    Allow,
    Rejected(IsDisconnect),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthResult {
    Allow(Superuser),
    ///User is not found
    NotFound,
    BadUsernameOrPassword,
    NotAuthorized,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageExpiryCheckResult {
    Expiry,
    Remaining(Option<NonZeroU32>),
}

impl MessageExpiryCheckResult {
    #[inline]
    pub fn is_expiry(&self) -> bool {
        matches!(self, Self::Expiry)
    }

    #[inline]
    pub fn message_expiry_interval(&self) -> Option<NonZeroU32> {
        match self {
            Self::Expiry => None,
            Self::Remaining(i) => *i,
        }
    }
}

#[inline]
pub fn parse_topic_filter(
    topic_filter: &ByteString,
    shared_subscription_supported: bool,
) -> Result<(TopicFilter, Option<SharedGroup>)> {
    let mut shared_group = None;
    let err = MqttError::TopicError("Illegal topic filter".into());
    //$share/abc/
    let topic = if shared_subscription_supported {
        let mut levels = topic_filter.splitn(3, '/').collect::<Vec<_>>();
        let is_share = levels.first().map(|f| *f == "$share").unwrap_or(false);
        if is_share {
            if levels.len() < 3 {
                return Err(err);
            }
            levels.remove(0);
            shared_group = Some(SharedGroup::from(levels.remove(0)));
            ByteString::from(levels.remove(0))
        } else {
            topic_filter.clone()
        }
    } else {
        topic_filter.clone()
    };
    if topic.is_empty() {
        return Err(err);
    }
    Ok((topic, shared_group))
}

#[derive(Clone, Debug)]
pub struct Subscribe {
    pub topic_filter: TopicFilter,
    pub qos: QoS,
    pub shared_group: Option<SharedGroup>,
}

impl Subscribe {
    #[inline]
    pub fn from_v3(topic_filter: &ByteString, qos: QoS, shared_subscription_supported: bool) -> Result<Self> {
        let (topic_filter, shared_group) = parse_topic_filter(topic_filter, shared_subscription_supported)?;
        Ok(Subscribe { topic_filter, qos, shared_group })
    }

    #[inline]
    pub fn from_v5(
        topic_filter: &ByteString,
        opt: &SubscriptionOptions,
        shared_subscription_supported: bool,
    ) -> Result<Self> {
        Subscribe::from_v3(topic_filter, opt.qos, shared_subscription_supported)
    }

    #[inline]
    pub fn is_shared(&self) -> bool {
        self.shared_group.is_some()
    }
}

#[derive(Clone, Debug)]
pub struct SubscribeReturn(pub SubscribeAckReason);

impl SubscribeReturn {
    #[inline]
    pub fn new_success(qos: QoS) -> Self {
        let status = match qos {
            QoS::AtMostOnce => SubscribeAckReason::GrantedQos0,
            QoS::AtLeastOnce => SubscribeAckReason::GrantedQos1,
            QoS::ExactlyOnce => SubscribeAckReason::GrantedQos2,
        };
        Self(status)
    }

    #[inline]
    pub fn new_failure(status: SubscribeAckReason) -> Self {
        Self(status)
    }

    #[inline]
    pub fn success(&self) -> Option<QoS> {
        match self.0 {
            SubscribeAckReason::GrantedQos0 => Some(QoS::AtMostOnce),
            SubscribeAckReason::GrantedQos1 => Some(QoS::AtLeastOnce),
            SubscribeAckReason::GrantedQos2 => Some(QoS::ExactlyOnce),
            _ => None,
        }
    }

    #[inline]
    pub fn failure(&self) -> bool {
        !matches!(
            self.0,
            SubscribeAckReason::GrantedQos0
                | SubscribeAckReason::GrantedQos1
                | SubscribeAckReason::GrantedQos2
        )
    }

    #[inline]
    pub fn into_inner(self) -> SubscribeAckReason {
        self.0
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SubscribedV5 {
    /// Packet Identifier
    pub packet_id: NonZeroU16,
    /// Subscription Identifier
    pub id: Option<NonZeroU32>,
    pub user_properties: UserProperties,
    /// the list of Topic Filters and QoS to which the Client wants to subscribe.
    pub topic_filter: (ByteString, SubscriptionOptions),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectAckReason {
    V3(ConnectAckReasonV3),
    V5(ConnectAckReasonV5),
}

impl ConnectAckReason {
    #[inline]
    pub fn success(&self) -> bool {
        matches!(
            *self,
            ConnectAckReason::V3(ConnectAckReasonV3::ConnectionAccepted)
                | ConnectAckReason::V5(ConnectAckReasonV5::Success)
        )
    }

    #[inline]
    pub fn not_authorized(&self) -> bool {
        matches!(
            *self,
            ConnectAckReason::V3(ConnectAckReasonV3::NotAuthorized)
                | ConnectAckReason::V3(ConnectAckReasonV3::BadUserNameOrPassword)
                | ConnectAckReason::V5(ConnectAckReasonV5::NotAuthorized)
                | ConnectAckReason::V5(ConnectAckReasonV5::BadUserNameOrPassword)
        )
    }

    #[inline]
    pub fn success_or_auth_error(&self) -> (bool, bool) {
        match *self {
            ConnectAckReason::V3(ConnectAckReasonV3::ConnectionAccepted)
            | ConnectAckReason::V5(ConnectAckReasonV5::Success) => (true, false),
            ConnectAckReason::V3(ConnectAckReasonV3::NotAuthorized)
            | ConnectAckReason::V3(ConnectAckReasonV3::BadUserNameOrPassword)
            | ConnectAckReason::V5(ConnectAckReasonV5::NotAuthorized)
            | ConnectAckReason::V5(ConnectAckReasonV5::BadUserNameOrPassword) => (false, true),
            _ => (false, false),
        }
    }

    #[inline]
    pub fn v3_error_ack<Io, St>(&self, handshake: v3::Handshake<Io>) -> HandshakeAckV3<Io, St> {
        match *self {
            ConnectAckReason::V3(ConnectAckReasonV3::UnacceptableProtocolVersion) => {
                handshake.service_unavailable()
            }
            ConnectAckReason::V3(ConnectAckReasonV3::IdentifierRejected) => handshake.identifier_rejected(),
            ConnectAckReason::V3(ConnectAckReasonV3::ServiceUnavailable) => handshake.service_unavailable(),
            ConnectAckReason::V3(ConnectAckReasonV3::BadUserNameOrPassword) => {
                handshake.bad_username_or_pwd()
            }
            ConnectAckReason::V3(ConnectAckReasonV3::NotAuthorized) => handshake.not_authorized(),
            ConnectAckReason::V3(ConnectAckReasonV3::Reserved) => handshake.service_unavailable(),
            _ => panic!("invalid value"),
        }
    }

    #[inline]
    pub fn v5_error_ack<Io, St>(&self, handshake: v5::Handshake<Io>) -> HandshakeAckV5<Io, St> {
        match *self {
            ConnectAckReason::V5(ack_reason) => handshake.failed(ack_reason),
            _ => panic!("invalid value"),
        }
    }

    #[inline]
    pub fn reason(&self) -> &'static str {
        match *self {
            ConnectAckReason::V3(r) => r.reason(),
            ConnectAckReason::V5(r) => r.reason(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Unsubscribe {
    pub topic_filter: TopicFilter,
    pub shared_group: Option<SharedGroup>,
}

impl Unsubscribe {
    #[inline]
    pub fn from(topic_filter: &ByteString, shared_subscription_supported: bool) -> Result<Self> {
        let (topic_filter, shared_group) = parse_topic_filter(topic_filter, shared_subscription_supported)?;
        Ok(Unsubscribe { topic_filter, shared_group })
    }

    #[inline]
    pub fn is_shared(&self) -> bool {
        self.shared_group.is_some()
    }
}

#[derive(Clone, Debug)]
pub enum UnsubscribeAck {
    V3,
    V5(UnsubscribeAckV5),
}

#[derive(Clone)]
pub enum LastWill<'a> {
    V3(&'a LastWillV3),
    V5(&'a LastWillV5),
}

impl<'a> fmt::Debug for LastWill<'a> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LastWill::V3(lw) => f
                .debug_struct("LastWill")
                .field("topic", &lw.topic)
                .field("retain", &lw.retain)
                .field("qos", &lw.qos.value())
                .field("message", &"<REDACTED>")
                .finish(),
            LastWill::V5(lw) => f
                .debug_struct("LastWill")
                .field("topic", &lw.topic)
                .field("retain", &lw.retain)
                .field("qos", &lw.qos.value())
                .field("message", &"<REDACTED>")
                .field("will_delay_interval_sec", &lw.will_delay_interval_sec)
                .field("correlation_data", &lw.correlation_data)
                .field("message_expiry_interval", &lw.message_expiry_interval)
                .field("content_type", &lw.content_type)
                .field("user_properties", &lw.user_properties)
                .field("is_utf8_payload", &lw.is_utf8_payload)
                .field("response_topic", &lw.response_topic)
                .finish(),
        }
    }
}

impl<'a> LastWill<'a> {
    #[inline]
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            LastWill::V3(lw) => {
                json!({
                    "qos": lw.qos.value(),
                    "retain": lw.retain,
                    "topic": lw.topic,
                    "message": base64::encode(lw.message.as_ref()),
                })
            }
            LastWill::V5(lw) => {
                json!({
                    "qos": lw.qos.value(),
                    "retain": lw.retain,
                    "topic": lw.topic,
                    "message": base64::encode(lw.message.as_ref()),

                    "will_delay_interval_sec": lw.will_delay_interval_sec,
                    "correlation_data": lw.correlation_data,
                    "message_expiry_interval": lw.message_expiry_interval,
                    "content_type": lw.content_type,
                    "user_properties": lw.user_properties,
                    "is_utf8_payload": lw.is_utf8_payload,
                    "response_topic": lw.response_topic,
                })
            }
        }
    }
}

impl<'a> Serialize for LastWill<'a> {
    #[inline]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            LastWill::V3(lw) => {
                let mut s = serializer.serialize_struct("LastWill", 4)?;
                s.serialize_field("qos", &lw.qos.value())?;
                s.serialize_field("retain", &lw.retain)?;
                s.serialize_field("topic", &lw.topic)?;
                s.serialize_field("message", &lw.message)?;
                s.end()
            }
            LastWill::V5(lw) => {
                let mut s = serializer.serialize_struct("LastWill", 11)?;
                s.serialize_field("qos", &lw.qos.value())?;
                s.serialize_field("retain", &lw.retain)?;
                s.serialize_field("topic", &lw.topic)?;
                s.serialize_field("message", &lw.message)?;

                s.serialize_field("will_delay_interval_sec", &lw.will_delay_interval_sec)?;
                s.serialize_field("correlation_data", &lw.correlation_data)?;
                s.serialize_field("message_expiry_interval", &lw.message_expiry_interval)?;
                s.serialize_field("content_type", &lw.content_type)?;
                s.serialize_field("user_properties", &lw.user_properties)?;
                s.serialize_field("is_utf8_payload", &lw.is_utf8_payload)?;
                s.serialize_field("response_topic", &lw.response_topic)?;

                s.end()
            }
        }
    }
}

#[derive(Clone, Debug)]
pub enum Sink {
    V3(MqttSinkV3),
    V5(MqttSinkV5),
}

impl Sink {
    #[inline]
    pub(crate) fn close(&self) {
        match self {
            Sink::V3(s) => {
                s.close();
            }
            Sink::V5(s) => s.close(),
        }
    }

    #[inline]
    pub(crate) fn publish(&self, p: &Publish, message_expiry_interval: Option<NonZeroU32>) -> Result<()> {
        let pkt = match self {
            Sink::V3(_) => p.into_v3(),
            Sink::V5(_) => p.into_v5(message_expiry_interval),
        };
        self.send(pkt)
    }

    #[inline]
    pub(crate) fn send(&self, p: Packet) -> Result<()> {
        match self {
            Sink::V3(s) => {
                if let Packet::V3(p) = p {
                    s.send(p)?;
                }
            }
            Sink::V5(s) => {
                if s.is_open() {
                    if let Packet::V5(p) = p {
                        s.send(p)?;
                    }
                } else {
                    return Err(MqttError::from(SendPacketError::Disconnected));
                }
            }
        }
        Ok(())
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Packet {
    V3(PacketV3),
    V5(PacketV5),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Default)]
pub struct PublishProperties {
    pub topic_alias: Option<NonZeroU16>,
    pub correlation_data: Option<Bytes>,
    pub message_expiry_interval: Option<NonZeroU32>,
    pub content_type: Option<ByteString>,
    pub user_properties: UserProperties,
    pub is_utf8_payload: Option<bool>,
    pub response_topic: Option<ByteString>,
    pub subscription_ids: Option<Vec<NonZeroU32>>,
}

impl std::convert::From<UserProperties> for PublishProperties {
    fn from(props: UserProperties) -> Self {
        PublishProperties {
            topic_alias: None,
            correlation_data: None,
            message_expiry_interval: None,
            content_type: None,
            user_properties: props,
            is_utf8_payload: None,
            response_topic: None,
            subscription_ids: None,
        }
    }
}

impl std::convert::From<PublishPropertiesV5> for PublishProperties {
    fn from(props: PublishPropertiesV5) -> Self {
        PublishProperties {
            topic_alias: props.topic_alias,
            correlation_data: props.correlation_data,
            message_expiry_interval: props.message_expiry_interval,
            content_type: props.content_type,
            user_properties: props.user_properties,
            is_utf8_payload: props.is_utf8_payload,
            response_topic: props.response_topic,
            subscription_ids: props.subscription_ids,
        }
    }
}

impl std::convert::From<PublishProperties> for PublishPropertiesV5 {
    fn from(props: PublishProperties) -> Self {
        PublishPropertiesV5 {
            topic_alias: props.topic_alias,
            correlation_data: props.correlation_data,
            message_expiry_interval: props.message_expiry_interval,
            content_type: props.content_type,
            user_properties: props.user_properties,
            is_utf8_payload: props.is_utf8_payload,
            response_topic: props.response_topic,
            subscription_ids: props.subscription_ids,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Publish {
    /// this might be re-delivery of an earlier attempt to send the Packet.
    pub dup: bool,
    pub retain: bool,
    /// the level of assurance for delivery of an Application Message.
    pub qos: QoS,
    /// the information channel to which payload data is published.
    pub topic: TopicName,
    /// only present in PUBLISH Packets where the QoS level is 1 or 2.
    pub packet_id: Option<NonZeroU16>,
    /// the Application Message that is being published.
    pub payload: Bytes,

    pub properties: PublishProperties,
    pub create_time: TimestampMillis,
}

impl<'a> std::convert::TryFrom<LastWill<'a>> for Publish {
    type Error = MqttError;

    #[inline]
    fn try_from(lw: LastWill<'a>) -> std::result::Result<Self, Self::Error> {
        let (retain, qos, topic, payload, user_props) = match lw {
            LastWill::V3(lw) => {
                let (topic, user_props) = if let Some(pos) = lw.topic.find('?') {
                    let topic = lw.topic.clone();
                    let query = lw.topic.as_bytes().slice(pos + 1..lw.topic.len());
                    let user_props = url::form_urlencoded::parse(query.as_ref())
                        .into_owned()
                        .map(|(key, val)| (ByteString::from(key), ByteString::from(val)))
                        .collect::<UserProperties>();
                    (topic, user_props)
                } else {
                    let topic = lw.topic.clone();
                    (topic, UserProperties::default())
                };

                (lw.retain, lw.qos, topic, lw.message.clone(), user_props)
            }
            LastWill::V5(lw) => {
                let topic = lw.topic.clone();
                (lw.retain, lw.qos, topic, lw.message.clone(), lw.user_properties.clone())
            }
        };

        Ok(Self {
            dup: false,
            retain,
            qos,
            topic,
            packet_id: None,
            payload,

            properties: PublishProperties::from(user_props),
            create_time: chrono::Local::now().timestamp_millis(),
        })
    }
}

impl std::convert::TryFrom<&v3::Publish> for Publish {
    type Error = MqttError;

    #[inline]
    fn try_from(p: &v3::Publish) -> std::result::Result<Self, Self::Error> {
        let query = p.query();
        let p_props = if !query.is_empty() {
            let user_props = url::form_urlencoded::parse(query.as_bytes())
                .into_owned()
                .map(|(key, val)| (ByteString::from(key), ByteString::from(val)))
                .collect::<UserProperties>();
            PublishProperties::from(user_props)
        } else {
            PublishProperties::default()
        };

        Ok(Self {
            dup: p.dup(),
            retain: p.retain(),
            qos: p.qos(),
            topic: TopicName::from(p.topic().path()),
            packet_id: p.id(),
            payload: p.take_payload(),

            properties: p_props,
            create_time: chrono::Local::now().timestamp_millis(),
        })
    }
}

impl std::convert::TryFrom<&v5::Publish> for Publish {
    type Error = MqttError;

    #[inline]
    fn try_from(p: &v5::Publish) -> std::result::Result<Self, Self::Error> {
        Ok(Self {
            dup: p.dup(),
            retain: p.retain(),
            qos: p.qos(),
            topic: TopicName::from(p.topic().path()),
            packet_id: p.id(),
            payload: p.take_payload(),

            properties: PublishProperties::from(p.packet().properties.clone()),
            create_time: chrono::Local::now().timestamp_millis(),
        })
    }
}

impl Publish {
    #[inline]
    pub fn into_v3(&self) -> Packet {
        let p = v3::codec::Publish {
            dup: self.dup,
            retain: self.retain,
            qos: self.qos,
            topic: self.topic.clone(),
            packet_id: self.packet_id,
            payload: self.payload.clone(),
        };
        Packet::V3(v3::codec::Packet::Publish(p))
    }

    #[inline]
    pub fn into_v5(&self, message_expiry_interval: Option<NonZeroU32>) -> Packet {
        let mut p = v5::codec::Publish {
            dup: self.dup,
            retain: self.retain,
            qos: self.qos,
            topic: self.topic.clone(),
            packet_id: self.packet_id,
            payload: self.payload.clone(),
            properties: self.properties.clone().into(),
        };
        p.properties.message_expiry_interval = message_expiry_interval;
        Packet::V5(v5::codec::Packet::Publish(p))
    }

    #[inline]
    pub fn payload(&self) -> &Bytes {
        &self.payload
    }

    #[inline]
    pub fn retain(&self) -> bool {
        self.retain
    }

    #[inline]
    pub fn topic(&self) -> &TopicName {
        &self.topic
    }

    #[inline]
    pub fn topic_mut(&mut self) -> &mut TopicName {
        &mut self.topic
    }

    #[inline]
    pub fn dup(&self) -> bool {
        self.dup
    }

    #[inline]
    pub fn set_dup(&mut self, b: bool) {
        self.dup = b
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.payload.is_empty()
    }

    #[inline]
    pub fn qos(&self) -> QoS {
        self.qos
    }

    #[inline]
    pub fn create_time(&self) -> TimestampMillis {
        self.create_time
    }

    #[inline]
    pub fn packet_id(&self) -> Option<PacketId> {
        self.packet_id.map(|id| id.get())
    }

    #[inline]
    pub fn packet_id_mut(&mut self) -> &mut Option<NonZeroU16> {
        &mut self.packet_id
    }

    #[inline]
    pub fn packet_id_is_none(&self) -> bool {
        self.packet_id.is_none()
    }

    #[inline]
    pub fn set_packet_id(&mut self, packet_id: PacketId) {
        self.packet_id = NonZeroU16::new(packet_id)
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub enum FromType {
    Custom,
    Admin,
    System,
    LastWill,
}

impl std::fmt::Display for FromType {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let typ = match self {
            FromType::Custom => "custom",
            FromType::Admin => "admin",
            FromType::System => "system",
            FromType::LastWill => "lastwill",
        };
        write!(f, "{}", typ)
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct From {
    typ: FromType,
    pub id: Id,
}

impl From {
    #[inline]
    pub fn from_custom(id: Id) -> From {
        From { typ: FromType::Custom, id }
    }

    #[inline]
    pub fn from_admin(id: Id) -> From {
        From { typ: FromType::Admin, id }
    }

    #[inline]
    pub fn from_system(id: Id) -> From {
        From { typ: FromType::System, id }
    }

    #[inline]
    pub fn from_lastwill(id: Id) -> From {
        From { typ: FromType::LastWill, id }
    }

    #[inline]
    pub fn typ(&self) -> FromType {
        self.typ
    }

    #[inline]
    pub fn is_system(&self) -> bool {
        matches!(self.typ, FromType::System)
    }

    #[inline]
    pub fn is_custom(&self) -> bool {
        matches!(self.typ, FromType::Custom)
    }

    #[inline]
    pub fn to_from_json(&self, json: serde_json::Value) -> serde_json::Value {
        let mut json = self.id.to_from_json(json);
        if let Some(obj) = json.as_object_mut() {
            obj.insert("from_type".into(), serde_json::Value::String(self.typ.to_string()));
        }
        json
    }
}

impl Deref for From {
    type Target = Id;
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.id
    }
}

impl std::fmt::Debug for From {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{:?}", self.id.to_string(), self.typ)
    }
}

//pub type From = Id;
pub type To = Id;

#[derive(Clone)]
pub struct Id(Arc<_Id>);

impl Id {
    #[inline]
    pub fn new(
        node_id: NodeId,
        local_addr: Option<SocketAddr>,
        remote_addr: Option<SocketAddr>,
        client_id: ClientId,
        username: Option<UserName>,
    ) -> Self {
        Self(Arc::new(_Id {
            node_id,
            local_addr,
            remote_addr,
            client_id,
            username,
            create_time: chrono::Local::now().timestamp_millis(),
        }))
    }

    #[inline]
    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "node": self.node(),
            "ipaddress": self.remote_addr,
            "clientid": self.client_id,
            "username": self.username_ref(),
            "create_time": self.create_time,
        })
    }

    #[inline]
    pub fn to_from_json(&self, mut json: serde_json::Value) -> serde_json::Value {
        if let Some(obj) = json.as_object_mut() {
            obj.insert("from_node".into(), serde_json::Value::Number(serde_json::Number::from(self.node())));
            obj.insert(
                "from_ipaddress".into(),
                self.remote_addr
                    .map(|a| serde_json::Value::String(a.to_string()))
                    .unwrap_or(serde_json::Value::Null),
            );
            obj.insert("from_clientid".into(), serde_json::Value::String(self.client_id.to_string()));
            obj.insert("from_username".into(), serde_json::Value::String(self.username_ref().into()));
        }
        json
    }

    #[inline]
    pub fn to_to_json(&self, mut json: serde_json::Value) -> serde_json::Value {
        if let Some(obj) = json.as_object_mut() {
            obj.insert("node".into(), serde_json::Value::Number(serde_json::Number::from(self.node())));
            obj.insert(
                "ipaddress".into(),
                self.remote_addr
                    .map(|a| serde_json::Value::String(a.to_string()))
                    .unwrap_or(serde_json::Value::Null),
            );
            obj.insert("clientid".into(), serde_json::Value::String(self.client_id.to_string()));
            obj.insert("username".into(), serde_json::Value::String(self.username_ref().into()));
        }
        json
    }

    #[inline]
    pub fn from(node_id: NodeId, client_id: ClientId) -> Self {
        Self::new(node_id, None, None, client_id, None)
    }

    #[inline]
    pub fn node(&self) -> NodeId {
        self.node_id
    }

    #[inline]
    pub fn username(&self) -> UserName {
        self.username.clone().unwrap_or_else(|| UserName::from_static(UNDEFINED))
    }

    #[inline]
    pub fn username_ref(&self) -> &str {
        self.username.as_ref().map(<UserName as AsRef<str>>::as_ref).unwrap_or_else(|| UNDEFINED)
    }
}

impl ToString for Id {
    #[inline]
    fn to_string(&self) -> String {
        format!(
            "{}@{}/{}/{}/{}",
            self.node_id,
            self.local_addr.map(|addr| addr.to_string()).unwrap_or_default(),
            self.remote_addr.map(|addr| addr.to_string()).unwrap_or_default(),
            self.client_id,
            self.username_ref()
        )
    }
}

impl std::fmt::Debug for Id {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}", self.to_string(), self.create_time)
    }
}

impl PartialEq<Id> for Id {
    #[inline]
    fn eq(&self, o: &Id) -> bool {
        self.node_id == o.node_id
            && self.client_id == o.client_id
            && self.local_addr == o.local_addr
            && self.remote_addr == o.remote_addr
            && self.username == o.username
            && self.create_time == o.create_time
    }
}

impl Eq for Id {}

impl std::hash::Hash for Id {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.node_id.hash(state);
        self.local_addr.hash(state);
        self.remote_addr.hash(state);
        self.client_id.hash(state);
        self.username.hash(state);
        self.create_time.hash(state);
    }
}

impl Deref for Id {
    type Target = _Id;
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Serialize for Id {
    #[inline]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        _Id::serialize(self.0.as_ref(), serializer)
    }
}

impl<'de> Deserialize<'de> for Id {
    #[inline]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Id(Arc::new(_Id::deserialize(deserializer)?)))
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Deserialize, Serialize)]
pub struct _Id {
    pub node_id: NodeId,
    pub local_addr: Option<SocketAddr>,
    pub remote_addr: Option<SocketAddr>,
    pub client_id: ClientId,
    pub username: Option<UserName>,
    pub create_time: TimestampMillis,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Retain {
    pub from: From,
    pub publish: Publish,
}

#[derive(Debug)]
pub enum Message {
    Forward(From, Publish),
    Kick(oneshot::Sender<()>, Id, CleanStart, IsAdmin),
    Disconnect(Disconnect),
    Closed(Reason),
    Keepalive,
    Subscribe(Subscribe, oneshot::Sender<Result<SubscribeReturn>>),
    Unsubscribe(Unsubscribe, oneshot::Sender<Result<()>>),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SessionStatus {
    pub id: Id,
    pub online: IsOnline,
    pub handshaking: bool,
}

#[derive(Deserialize, Serialize, Debug, Default, Clone)]
pub struct SubsSearchParams {
    #[serde(default)]
    pub _limit: usize,
    pub clientid: Option<String>,
    pub topic: Option<String>,
    //value is 0,1,2
    pub qos: Option<u8>,
    pub share: Option<SharedGroup>,
    pub _match_topic: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Default)]
pub struct SubsSearchResult {
    pub node_id: NodeId,
    pub clientid: ClientId,
    pub client_addr: Option<SocketAddr>,
    pub topic: TopicFilter,
    pub qos: u8,
    pub share: Option<SharedGroup>,
}

#[derive(Deserialize, Serialize, Debug, Default, PartialEq, Eq, Hash, Clone)]
pub struct Route {
    pub node_id: NodeId,
    pub topic: TopicFilter,
}

pub struct SessionSubs(Arc<_SessionSubs>);

impl SessionSubs {
    #[inline]
    pub(crate) fn new() -> Self {
        Self(Arc::new(_SessionSubs::new()))
    }
}

impl Deref for SessionSubs {
    type Target = _SessionSubs;
    #[inline]
    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

pub struct _SessionSubs {
    subs: DashMap<TopicFilter, SubscriptionValue>,
}

impl _SessionSubs {
    #[inline]
    pub(crate) fn new() -> Self {
        Self { subs: DashMap::default() }
    }

    #[inline]
    pub fn add(&self, topic_filter: TopicFilter, qos: QoS, shared_group: Option<SharedGroup>) {
        let is_shared = shared_group.is_some();
        let prev = self.subs.insert(topic_filter, (qos, shared_group));

        if let Some((_, prev_group)) = prev {
            match (prev_group.is_some(), is_shared) {
                (true, false) => {
                    Runtime::instance().stats.subscriptions_shared.dec();
                }
                (false, true) => {
                    Runtime::instance().stats.subscriptions_shared.inc();
                }
                (false, false) => {}
                (true, true) => {}
            }
        } else {
            Runtime::instance().stats.subscriptions.inc();
            if is_shared {
                Runtime::instance().stats.subscriptions_shared.inc();
            }
        }
    }

    #[inline]
    pub fn remove(&self, topic_filter: &str) -> Option<(TopicFilter, SubscriptionValue)> {
        let removed = self.subs.remove(topic_filter);
        if let Some((_, (_, group))) = &removed {
            Runtime::instance().stats.subscriptions.dec();
            if group.is_some() {
                Runtime::instance().stats.subscriptions_shared.dec();
            }
        }
        removed
    }

    #[inline]
    pub fn drain(&self) -> Subscriptions {
        let topic_filters = self.subs.iter().map(|entry| entry.key().clone()).collect::<Vec<_>>();
        let subs = topic_filters.iter().filter_map(|tf| self.remove(tf)).collect();
        subs
    }

    #[inline]
    pub fn extend(&self, subs: Subscriptions) {
        for (topic_filter, (qos, group)) in subs {
            self.add(topic_filter, qos, group);
        }
    }

    #[inline]
    pub fn clear(&self) {
        for entry in self.subs.iter() {
            Runtime::instance().stats.subscriptions.dec();
            let (_, group) = entry.value();
            if group.is_some() {
                Runtime::instance().stats.subscriptions_shared.dec();
            }
        }
        self.subs.clear();
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.subs.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn to_topic_filters(&self) -> TopicFilters {
        self.subs.iter().map(|entry| TopicFilter::from(entry.key().as_ref())).collect()
    }

    #[inline]
    pub fn iter(
        &self,
    ) -> dashmap::iter::Iter<
        TopicFilter,
        SubscriptionValue,
        ahash::RandomState,
        DashMap<TopicFilter, SubscriptionValue>,
    > {
        self.subs.iter()
    }
}

pub struct ExtraAttrs {
    attrs: HashMap<String, Box<dyn Any + Sync + Send>>,
}

impl Default for ExtraAttrs {
    fn default() -> Self {
        Self::new()
    }
}

impl ExtraAttrs {
    #[inline]
    pub fn new() -> Self {
        Self { attrs: HashMap::default() }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.attrs.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.attrs.is_empty()
    }

    #[inline]
    pub fn clear(&mut self) {
        self.attrs.clear()
    }

    #[inline]
    pub fn insert<T: Any + Sync + Send>(&mut self, key: String, value: T) {
        self.attrs.insert(key, Box::new(value));
    }

    #[inline]
    pub fn get<T: Any + Sync + Send>(&self, key: &str) -> Option<&T> {
        self.attrs.get(key).and_then(|v| v.downcast_ref::<T>())
    }

    #[inline]
    pub fn get_mut<T: Any + Sync + Send>(&mut self, key: &str) -> Option<&mut T> {
        self.attrs.get_mut(key).and_then(|v| v.downcast_mut::<T>())
    }

    #[inline]
    pub fn get_default_mut<T: Any + Sync + Send, F: Fn() -> T>(
        &mut self,
        key: String,
        def_fn: F,
    ) -> Option<&mut T> {
        self.attrs.entry(key).or_insert_with(|| Box::new(def_fn())).downcast_mut::<T>()
    }
}

#[derive(Clone, Debug)]
pub struct TimedValue<V>(V, Option<Instant>);

impl<V> TimedValue<V> {
    pub fn new(value: V, timeout_duration: Option<Duration>) -> Self {
        TimedValue(value, timeout_duration.map(|t| Instant::now() + t))
    }

    pub fn value(&self) -> &V {
        &self.0
    }

    pub fn value_mut(&mut self) -> &mut V {
        &mut self.0
    }

    pub fn into_value(self) -> V {
        self.0
    }

    pub fn is_expired(&self) -> bool {
        self.1.map(|e| Instant::now() >= e).unwrap_or(false)
    }
}

impl<V> PartialEq for TimedValue<V>
where
    V: PartialEq,
{
    fn eq(&self, other: &TimedValue<V>) -> bool {
        self.value() == other.value()
    }
}

//impl<V> Eq for TimedValue<V> where V: Eq {}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct StateFlags: u8 {
        const Kicked = 0b00000001;
        const ByAdminKick = 0b00000010;
        const DisconnectReceived = 0b00000100;
        const CleanStart = 0b00001000;
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum Reason {
    ConnectDisconnect(Option<ByteString>),
    ConnectReadWriteTimeout,
    ConnectReadWriteError,
    ConnectRemoteClose,
    ConnectKeepaliveTimeout,
    ConnectKicked(IsAdmin),
    SessionExpiration,
    SubscribeFailed(Option<ByteString>),
    UnsubscribeFailed(Option<ByteString>),
    SubscribeRefused,
    PublishRefused,
    MessageExpiration,
    MessageQueueFull,
    PublishFailed(ByteString),
    ProtocolError(ByteString),
    Error(ByteString),
    Reasons(Vec<Reason>),
    Unknown,
}

impl Default for Reason {
    #[inline]
    fn default() -> Self {
        Reason::Unknown
    }
}

impl Reason {
    #[inline]
    pub fn from_static(r: &'static str) -> Self {
        Reason::Error(ByteString::from_static(r))
    }

    #[inline]
    pub fn is_kicked(&self, admin_opt: IsAdmin) -> bool {
        match self {
            Reason::ConnectKicked(_admin_opt) => *_admin_opt == admin_opt,
            _ => false,
        }
    }

    #[inline]
    pub fn is_kicked_by_admin(&self) -> bool {
        matches!(self, Reason::ConnectKicked(true))
    }
}

impl std::convert::From<&str> for Reason {
    #[inline]
    fn from(r: &str) -> Self {
        Reason::Error(ByteString::from(r))
    }
}

impl std::convert::From<String> for Reason {
    #[inline]
    fn from(r: String) -> Self {
        Reason::Error(ByteString::from(r))
    }
}

impl std::convert::From<MqttError> for Reason {
    #[inline]
    fn from(e: MqttError) -> Self {
        match e {
            MqttError::Reason(r) => r,
            MqttError::SendPacketError(_) | MqttError::IoError(_) => Reason::ConnectReadWriteError,
            MqttError::Timeout(_) => Reason::ConnectReadWriteTimeout,
            MqttError::None => Reason::Unknown,
            _ => Reason::Error(ByteString::from(e.to_string())),
        }
    }
}

impl Display for Reason {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let r = match self {
            Reason::ConnectDisconnect(r) => {
                //Disconnect message received
                match r {
                    Some(r) => return write!(f, "Disconnect({})", r),
                    None => "Disconnect",
                }
            }
            Reason::ConnectReadWriteTimeout => {
                "ReadWriteTimeout" //read/write timeout
            }
            Reason::ConnectReadWriteError => {
                "ReadWriteError" //read/write error
            }
            Reason::ConnectRemoteClose => {
                "RemoteClose" //"connection close by remote client"
            }
            Reason::ConnectKeepaliveTimeout => {
                "KeepaliveTimeout" //keepalive timeout
            }
            Reason::ConnectKicked(admin_opt) => {
                if *admin_opt {
                    "ByAdminKick" //kicked by administrator
                } else {
                    "Kicked" //kicked
                }
            }
            Reason::SessionExpiration => {
                "SessionExpiration" //session expiration
            }
            Reason::SubscribeFailed(r) => {
                //subscribe failed
                match r {
                    Some(r) => return write!(f, "SubscribeFailed({})", r),
                    None => "SubscribeFailed",
                }
            }
            Reason::UnsubscribeFailed(r) => {
                //unsubscribe failed
                match r {
                    Some(r) => return write!(f, "UnsubscribeFailed({})", r),
                    None => "UnsubscribeFailed",
                }
            }
            Reason::SubscribeRefused => {
                "SubscribeRefused" //subscribe refused
            }
            Reason::PublishRefused => {
                "PublishRefused" //publish refused
            }
            Reason::MessageExpiration => {
                "MessageExpiration" //message expiration
            }
            Reason::MessageQueueFull => {
                "MessageQueueFull" //message deliver queue is full
            }
            Reason::PublishFailed(r) => return write!(f, "PublishFailed({})", r),
            Reason::Error(r) => r,
            Reason::ProtocolError(r) => return write!(f, "ProtocolError({})", r),
            Reason::Reasons(reasons) => match reasons.len() {
                0 => "",
                1 => return write!(f, "{}", reasons.get(0).map(|r| r.to_string()).unwrap_or_default()),
                _ => return write!(f, "{}", reasons.iter().map(|r| r.to_string()).join(",")),
            },
            Reason::Unknown => {
                "Unknown" //unknown
            }
        };
        write!(f, "{}", r)
    }
}

#[test]
fn test_reason() {
    assert_eq!(Reason::ConnectKicked(false).is_kicked(false), true);
    assert_eq!(Reason::ConnectKicked(false).is_kicked(true), false);
    assert_eq!(Reason::ConnectKicked(true).is_kicked(true), true);
    assert_eq!(Reason::ConnectKicked(true).is_kicked(false), false);
    assert_eq!(Reason::ConnectKicked(true).is_kicked_by_admin(), true);
    assert_eq!(Reason::ConnectKicked(false).is_kicked_by_admin(), false);
    assert_eq!(Reason::ConnectDisconnect(None).is_kicked(false), false);
    assert_eq!(Reason::ConnectDisconnect(None).is_kicked_by_admin(), false);

    let reasons = Reason::Reasons(vec![
        Reason::PublishRefused,
        Reason::ConnectKicked(false),
        Reason::MessageExpiration,
    ]);
    assert_eq!(reasons.to_string(), "PublishRefused,Kicked,MessageExpiration");
}
