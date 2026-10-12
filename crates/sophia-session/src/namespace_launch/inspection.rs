//! Dispatch published inspection names to their captured production owner.
use super::*;
use sophia_9p::{ConnectionId, Errno};
use sophia_protocol::namespace_view::ServiceReference;
use sophia_runtime::{
    authenticated_files::FileAuthentication,
    inspection::{INSPECTION_IDLE_CHECK, InspectionAcceptor, InspectionConnection},
};
use std::sync::atomic::AtomicBool;
use std::thread::JoinHandle;

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

pub(super) struct InspectionWorker {
    stop: Arc<AtomicBool>,
    normal_exit: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl InspectionWorker {
    pub(super) fn finished(&self) -> bool {
        self.thread.as_ref().is_some_and(JoinHandle::is_finished)
    }
    pub(super) fn failed(&self) -> bool {
        self.finished() && !self.normal_exit.load(Ordering::Acquire)
    }
}
impl Drop for InspectionWorker {
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
        if !self
            .endpoints
            .iter()
            .any(|e| e.listener.endpoint.endpoint.service == *service)
        {
            return Err("inspection service absent from client view".into());
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
            .name("sophia-recipe-inspection".into())
            .spawn(move || {
                completion.store(
                    run(listeners, acceptor, stopping).is_ok(),
                    Ordering::Release,
                );
                wake.notify();
            })
            .map_err(|e| e.to_string())?;
        self.services.push(InspectionWorker {
            stop,
            normal_exit,
            thread: Some(thread),
        });
        Ok(())
    }
}

fn run(
    mut listeners: Vec<Listener>,
    acceptor: InspectionAcceptor,
    stop: Arc<AtomicBool>,
) -> std::io::Result<()> {
    let mut connections: Vec<InspectionConnection> = Vec::new();
    while !stop.load(Ordering::Acquire) && acceptor.is_live() {
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
                    if let Ok(connection) =
                        acceptor.adopt(stream, Box::new(listener.factory.clone()))
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
