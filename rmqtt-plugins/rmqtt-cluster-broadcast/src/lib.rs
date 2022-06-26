#[macro_use]
extern crate async_trait;
#[macro_use]
extern crate serde;
#[macro_use]
extern crate serde_json;

use std::sync::Arc;

use futures::future::FutureExt;
use parking_lot::RwLock;

use config::PluginConfig;
use handler::HookHandler;
use retainer::ClusterRetainer;
use rmqtt::{
    broker::{
        error::MqttError,
        hook::{Register, Type},
        session::SessionOfflineInfo,
        types::{From, Publish, Reason, To},
    },
    grpc::{client::NodeGrpcClient, Message, MessageReply, MessageType, GrpcClients},
    plugin::{DynPlugin, DynPluginResult, Plugin},
    Result, Runtime,
};
use shared::ClusterShared;

mod config;
mod handler;
mod retainer;
mod shared;

type HashMap<K, V> = std::collections::HashMap<K, V, ahash::RandomState>;

#[inline]
pub async fn register(
    runtime: &'static Runtime,
    name: &'static str,
    descr: &'static str,
    default_startup: bool,
    immutable: bool,
) -> Result<()> {
    runtime
        .plugins
        .register(name, default_startup, immutable, move || -> DynPluginResult {
            Box::pin(async move {
                ClusterPlugin::new(runtime, name, descr).await.map(|p| -> DynPlugin { Box::new(p) })
            })
        })
        .await?;
    Ok(())
}

struct ClusterPlugin {
    runtime: &'static Runtime,
    name: String,
    descr: String,
    register: Box<dyn Register>,
    cfg: Arc<RwLock<PluginConfig>>,
    grpc_clients: GrpcClients,
    shared: &'static ClusterShared,
    retainer: &'static ClusterRetainer,
    // shared_subscriber: &'static ClusterSharedSubscriber,
}

impl ClusterPlugin {
    #[inline]
    async fn new<S: Into<String>>(runtime: &'static Runtime, name: S, descr: S) -> Result<Self> {
        let name = name.into();
        let cfg = Arc::new(RwLock::new(
            runtime
                .settings
                .plugins
                .load_config::<PluginConfig>(&name)
                .map_err(|e| MqttError::from(e.to_string()))?,
        ));
        log::debug!("{} ClusterPlugin cfg: {:?}", name, cfg.read());

        let register = runtime.extends.hook_mgr().await.register();
        let mut grpc_clients = HashMap::default();
        let node_grpc_addrs = cfg.read().node_grpc_addrs.clone();
        for node_addr in &node_grpc_addrs {
            if node_addr.id != runtime.node.id() {
                grpc_clients.insert(node_addr.id, (node_addr.addr, runtime.node.new_grpc_client(&node_addr.addr).await?));
            }
        }
        let grpc_clients = Arc::new(grpc_clients);
        let message_type = cfg.read().message_type;
        let shared = ClusterShared::get_or_init(grpc_clients.clone(), message_type);
        let retainer = ClusterRetainer::get_or_init(grpc_clients.clone(), message_type);
        // let shared_subscriber =
        //     ClusterSharedSubscriber::get_or_init(grpc_clients.clone(), runtime.node.id(), message_type);
        Ok(Self { runtime, name, descr: descr.into(), register, cfg, grpc_clients, shared, retainer })
    }
}

#[async_trait]
impl Plugin for ClusterPlugin {
    #[inline]
    async fn init(&mut self) -> Result<()> {
        log::info!("{} init", self.name);
        self.register
            .add(Type::GrpcMessageReceived, Box::new(HookHandler::new(self.shared, self.retainer)))
            .await;
        Ok(())
    }

    #[inline]
    fn name(&self) -> &str {
        &self.name
    }

    #[inline]
    async fn get_config(&self) -> Result<serde_json::Value> {
        self.cfg.read().to_json()
    }

    #[inline]
    async fn start(&mut self) -> Result<()> {
        log::info!("{} start", self.name);
        self.register.start().await;
        *self.runtime.extends.shared_mut().await = Box::new(self.shared);
        *self.runtime.extends.retain_mut().await = Box::new(self.retainer);
        // *self.runtime.extends.shared_subscriber_mut().await = Box::new(self.shared_subscriber);
        Ok(())
    }

    #[inline]
    async fn stop(&mut self) -> Result<bool> {
        log::warn!("{} stop, once the cluster is started, it cannot be stopped", self.name);
        Ok(false)
    }

    #[inline]
    fn version(&self) -> &str {
        "0.1.1"
    }

    #[inline]
    fn descr(&self) -> &str {
        &self.descr
    }

    #[inline]
    async fn attrs(&self) -> serde_json::Value {
        let mut nodes = HashMap::default();
        for (id, (addr, c)) in self.grpc_clients.iter() {
            let stats = json!({
                "channel_tasks": c.channel_tasks(),
                "active_tasks": c.active_tasks(),
            });
            nodes.insert(format!("{}/{:?}", id, addr), stats);
        }
        json!({
            "grpc_clients": nodes,
        })
    }
}

pub struct MessageBroadcaster {
    grpc_clients: GrpcClients,
    msg_type: MessageType,
    msg: Option<Message>,
}

impl MessageBroadcaster {
    pub fn new(grpc_clients: GrpcClients, msg_type: MessageType, msg: Message) -> Self {
        Self { grpc_clients, msg_type, msg: Some(msg) }
    }

    #[inline]
    pub async fn join_all(&mut self) -> Vec<Result<MessageReply>> {
        let msg = self.msg.take().unwrap();
        let mut senders = Vec::new();
        let max_idx = self.grpc_clients.len() - 1;
        for (i, (_, (_, grpc_client))) in self.grpc_clients.iter().enumerate() {
            if i == max_idx {
                senders.push(grpc_client.send_message(self.msg_type, msg));
                break;
            } else {
                senders.push(grpc_client.send_message(self.msg_type, msg.clone()));
            }
        }
        futures::future::join_all(senders).await
    }

    #[inline]
    pub async fn kick(&mut self) -> Result<SessionOfflineInfo> {
        let reply = self
            .select_ok(&|reply: MessageReply| -> Result<MessageReply> {
                log::debug!("reply: {:?}", reply);
                if let MessageReply::Kick(Some(o)) = reply {
                    Ok(MessageReply::Kick(Some(o)))
                } else {
                    Err(MqttError::None)
                }
            })
            .await?;
        if let MessageReply::Kick(Some(kicked)) = reply {
            Ok(kicked)
        } else {
            Err(MqttError::None)
        }
    }

    #[inline]
    pub async fn select_ok<F: Fn(MessageReply) -> Result<MessageReply> + Send + Sync>(
        &mut self,
        check_fn: &F,
    ) -> Result<MessageReply> {
        let msg = self.msg.take().unwrap();
        let mut senders = Vec::new();
        let max_idx = self.grpc_clients.len() - 1;
        for (i, (_, (_, grpc_client))) in self.grpc_clients.iter().enumerate() {
            if i == max_idx {
                senders.push(Self::send(grpc_client, self.msg_type, msg, check_fn).boxed());
                break;
            } else {
                senders.push(Self::send(grpc_client, self.msg_type, msg.clone(), check_fn).boxed());
            }
        }
        let (reply, _) = futures::future::select_ok(senders).await?;
        Ok(reply)
    }

    async fn send<F: Fn(MessageReply) -> Result<MessageReply> + Send + Sync>(
        grpc_client: &NodeGrpcClient,
        typ: MessageType,
        msg: Message,
        check_fn: &F,
    ) -> Result<MessageReply> {
        match grpc_client.send_message(typ, msg).await {
            Ok(r) => {
                log::debug!("OK reply: {:?}", r);
                check_fn(r)
            }
            Err(e) => {
                log::debug!("ERROR reply: {:?}", e);
                Err(e)
            }
        }
    }
}

pub(crate) async fn hook_message_dropped(droppeds: Vec<(To, From, Publish, Reason)>) {
    for (to, from, publish, reason) in droppeds {
        //hook, message_dropped
        Runtime::instance().extends.hook_mgr().await.message_dropped(Some(to), from, publish, reason).await;
    }
}
