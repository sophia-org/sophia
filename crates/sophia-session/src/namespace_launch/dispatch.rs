//! Bounded dispatch from published names to their selected role owners.
use super::*;
use sophia_9p::{ConnectionId, Errno};
use sophia_portal::{
    client_files::PortalClientConnection, recipient_object::PortalObjectRecipient,
};
use sophia_protocol::namespace_view::ServiceReference;
use sophia_runtime::{
    authenticated_files::FileAuthentication,
    inspection::{INSPECTION_IDLE_CHECK, InspectionAcceptor, InspectionConnection},
};
use std::sync::atomic::AtomicBool;
use std::thread::JoinHandle;

enum Owner {
    Inspection(InspectionAcceptor),
    Portal {
        objects: Arc<PortalObjectRecipient>,
        epoch: u64,
    },
}
enum Connection {
    Inspection(InspectionConnection),
    Portal(PortalClientConnection),
}
impl Owner {
    fn is_live(&self) -> bool {
        match self {
            Self::Inspection(owner) => owner.is_live(),
            Self::Portal { .. } => true,
        }
    }
    fn adopt(
        &self,
        stream: std::os::unix::net::UnixStream,
        factory: Box<dyn FileAuthenticationFactory>,
    ) -> std::io::Result<Connection> {
        match self {
            Self::Inspection(owner) => owner.adopt(stream, factory).map(Connection::Inspection),
            Self::Portal { objects, epoch } => PortalClientConnection::adopt(
                stream,
                objects.namespace(),
                objects.clone(),
                *epoch,
                factory,
            )
            .map(Connection::Portal),
        }
    }
}
impl Connection {
    fn poll(&mut self) -> std::io::Result<bool> {
        match self {
            Self::Inspection(connection) => connection.poll(),
            Self::Portal(connection) => connection.poll(),
        }
    }
}

#[derive(Clone)]
struct SharedFactory(Arc<Mutex<Box<dyn FileAuthenticationFactory>>>);
impl FileAuthenticationFactory for SharedFactory {
    fn create(
        &mut self,
        connection: ConnectionId,
        now: Instant,
        wake: sophia_9p::unix::Wake,
    ) -> Result<Box<dyn FileAuthentication>, Errno> {
        self.0
            .try_lock()
            .map_err(|_| Errno::EAGAIN)?
            .create(connection, now, wake)
    }
    fn retire(
        &mut self,
        owner: Box<dyn FileAuthentication>,
    ) -> Result<(), Box<dyn FileAuthentication>> {
        match self.0.try_lock() {
            Ok(mut factory) => factory.retire(owner),
            Err(_) => Err(owner),
        }
    }
}
struct Listener {
    endpoint: NamespaceListener,
    _view: NamespaceViewLease,
    grant: sophia_runtime::EndpointGrantLease,
    factory: SharedFactory,
}

pub(super) struct DispatchWorker {
    stop: Arc<AtomicBool>,
    normal_exit: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl DispatchWorker {
    pub(super) fn finished(&self) -> bool {
        self.thread.as_ref().is_some_and(JoinHandle::is_finished)
    }
    pub(super) fn failed(&self) -> bool {
        self.finished() && !self.normal_exit.load(Ordering::Acquire)
    }
}
impl Drop for DispatchWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl RunningNamespaceClient {
    /// Session matches an exact service reference, not merely a pathname or
    /// interface label. Other role endpoints remain available for their owner.
    /// Retirement joins this bounded worker off the Session thread.
    pub fn serve_inspection(
        &mut self,
        service: &ServiceReference,
        acceptor: InspectionAcceptor,
        wake: sophia_wake::Notifier,
    ) -> Result<(), String> {
        if service.protocol != EndpointProtocol::NineP2000L
            || service.interface != "inspection"
            || service.version != 1
            || service.generation != acceptor.generation()
            || !acceptor.is_live()
        {
            return Err("inspection dispatch service mismatch".into());
        }
        self.serve_role(service, Owner::Inspection(acceptor), wake)
    }

    /// Recipient access only. The trusted coordinator installs objects; this
    /// client endpoint has no broker, upload or execution authority.
    pub fn serve_portal(
        &mut self,
        service: &ServiceReference,
        objects: Arc<PortalObjectRecipient>,
        wake: sophia_wake::Notifier,
    ) -> Result<(), String> {
        if !service.is_valid()
            || service.protocol != EndpointProtocol::NineP2000L
            || service.interface != "portal-client"
            || service.version != 1
            || self
                .endpoints
                .iter()
                .filter(|e| e.listener.endpoint.endpoint.service == *service)
                .any(|e| e.grant.endpoint().scope.namespace != objects.namespace())
        {
            return Err("portal dispatch service mismatch".into());
        }
        self.serve_role(
            service,
            Owner::Portal {
                objects,
                epoch: service.generation,
            },
            wake,
        )
    }

    fn serve_role(
        &mut self,
        service: &ServiceReference,
        owner: Owner,
        wake: sophia_wake::Notifier,
    ) -> Result<(), String> {
        if !self
            .endpoints
            .iter()
            .any(|e| e.listener.endpoint.endpoint.service == *service)
        {
            return Err("service absent from client view".into());
        }
        let mut listeners = Vec::new();
        let mut index = 0;
        while index < self.endpoints.len() {
            if self.endpoints[index].listener.endpoint.endpoint.service != *service {
                index += 1;
                continue;
            }
            self.endpoints[index]
                .listener
                .listener
                .set_nonblocking(true)
                .map_err(|e| e.to_string())?;
            let endpoint = self.endpoints.remove(index);
            listeners.push(Listener {
                endpoint: endpoint.listener,
                _view: endpoint.view,
                grant: endpoint.grant,
                factory: SharedFactory(Arc::new(Mutex::new(endpoint.factory))),
            });
        }
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let normal_exit = Arc::new(AtomicBool::new(false));
        let completion = normal_exit.clone();
        let thread = std::thread::Builder::new()
            .name("sophia-recipe-role".into())
            .spawn(move || {
                completion.store(run(listeners, owner, stopping).is_ok(), Ordering::Release);
                wake.notify();
            })
            .map_err(|e| e.to_string())?;
        self.services.push(DispatchWorker {
            stop,
            normal_exit,
            thread: Some(thread),
        });
        Ok(())
    }
}

fn run(mut listeners: Vec<Listener>, owner: Owner, stop: Arc<AtomicBool>) -> std::io::Result<()> {
    let mut connections: Vec<Connection> = Vec::new();
    while !stop.load(Ordering::Acquire) && owner.is_live() {
        connections.retain_mut(|connection| connection.poll().unwrap_or(false));
        listeners.retain(|listener| listener.grant.is_live());
        if listeners.is_empty() {
            break;
        }
        // At most one accept per listener per turn. The production inspection
        // owner and shared authentication budget bound accepted connections.
        for listener in &listeners {
            match listener.endpoint.listener.accept() {
                Ok((stream, _)) => {
                    if let Ok(connection) = owner.adopt(stream, Box::new(listener.factory.clone()))
                    {
                        connections.push(connection);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error),
            }
        }
        std::thread::sleep(INSPECTION_IDLE_CHECK);
    }
    Ok(())
}
