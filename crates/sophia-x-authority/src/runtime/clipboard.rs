impl XAuthorityRuntime {
     pub(crate) fn register_portal_selection(&mut self, selection: crate::XAtom,
         offer: crate::XPortalSelectionOffer) {
         self.external_clipboard_offers.insert(selection, offer);
     }

     fn prepare_external_clipboard_request(&self, target_namespace: NamespaceId,
         request: crate::XSelectionRequest, transfer: sophia_protocol::PortalTransferId,
         offer: crate::XPortalSelectionOffer) -> Result<ClipboardSelectionDispatch, crate::ClipboardSelectionRequestError> {
         if offer.source_namespace == target_namespace {
             return Err(crate::ClipboardSelectionRequestError::SameNamespace);
         }
         let portal_request = crate::ClipboardSelectionPortalRequest {
             request: sophia_portal::ClipboardTransferRequest {transfer,
                 source_namespace: offer.source_namespace, target_namespace,
                 target: sophia_portal::ClipboardTarget::Atom(request.target_name),
                 byte_size: 0, generation: offer.generation},
             failure: ClipboardSelectionFailureRequest {transfer, requestor: request.requestor,
                 selection: request.selection, target: request.target, time: request.time},
             property: request.property,
         };
         let command = sophia_portal::PortalCommand::PromptClipboardTransfer(
             sophia_portal::ClipboardPortal::prepare_import(&portal_request.request)
                 .map_err(crate::ClipboardSelectionRequestError::Portal)?);
         Ok(ClipboardSelectionDispatch::CrossNamespace {portal_request, command})
     }
     pub(crate) fn clipboard_selection_generation(&self, namespace: NamespaceId, selection: crate::XAtom) -> Option<u64> {
         self.selections.owner(selection, Some(namespace))
             .filter(|record| record.owner.is_some()).map(|record| record.generation)
     }
     /// Resolve a retained conversion to the current owner of its exact X
     /// selection. PRIMARY and CLIPBOARD may have different generations in
     /// the same namespace; a requester cannot choose either by naming a
     /// namespace or supplying a newer generation.
     pub fn clipboard_source_generation(
         &self,
         request: &sophia_protocol::PortalRequest,
     ) -> Option<u64> {
         let transfer = &request.transfer;
         let pending = self.pending_clipboard.get(&transfer.transfer)?;
         let retained = &pending.portal_request.request;
         if transfer.kind != sophia_protocol::PortalTransferKind::Clipboard
             || transfer.source_namespace != retained.source_namespace
             || transfer.target_namespace != retained.target_namespace
             || transfer.generation != retained.generation
             || transfer.byte_size != retained.byte_size
             || transfer.mime_type.as_deref() != Some(retained.target.as_str())
         {
             return None;
         }
         self.selections
             .owner(pending.portal_request.failure.selection, Some(retained.source_namespace))
             .filter(|record| record.owner.is_some())
             .map(|record| record.generation)
     }

     /// Returns the core X11 selection owner visible inside one admitted
     /// namespace. Classic shared-X clients use the same namespace; confined
     /// clients cannot discover an owner from another namespace.
     pub fn selection_owner(
         &self,
         namespace: NamespaceId,
         selection: crate::XAtom,
     ) -> Option<crate::XResourceId> {
         self.selections
             .owner(selection, Some(namespace))
             .and_then(|record| record.owner)
     }

     pub(crate) fn set_pending_clipboard_byte_order(
         &mut self,
         transfer: sophia_protocol::PortalTransferId,
         byte_order: XByteOrder,
     ) {
         if let Some(pending) = self.pending_clipboard.get_mut(&transfer) {
             pending.byte_order = byte_order;
         }
     }
 
     pub fn begin_clipboard_source_request(
         &mut self,
         grant: &sophia_protocol::PortalGrant,
     ) -> Result<ClipboardSelectionProxy, ClipboardSelectionExecutionError> {
         let pending = self
             .pending_clipboard
             .get(&grant.transfer)
             .ok_or(ClipboardSelectionExecutionError::UnknownTransfer)?;
         if grant.state != sophia_protocol::PortalGrantState::Active
             || grant.source_generation != pending.portal_request.request.generation
             || grant.source_namespace != pending.portal_request.request.source_namespace
             || grant.target_namespace != pending.portal_request.request.target_namespace
         {
             return Err(ClipboardSelectionExecutionError::StaleOwnerGeneration);
         }
         let failure = pending.portal_request.failure;
         self.begin_clipboard_source_selection(grant, failure.selection, failure.target, failure.time)
     }

     pub(crate) fn begin_clipboard_source_selection(
         &mut self,
         grant: &sophia_protocol::PortalGrant,
         selection: crate::XAtom,
         target: crate::XAtom,
         time: crate::XTimestamp,
     ) -> Result<ClipboardSelectionProxy, ClipboardSelectionExecutionError> {
         if grant.kind != sophia_protocol::PortalTransferKind::Clipboard
             || grant.state != sophia_protocol::PortalGrantState::Active {
             return Err(ClipboardSelectionExecutionError::Denied);
         }
         let owner_record = self
             .selections
             .owner(
                 selection,
                 Some(grant.source_namespace),
             )
             .filter(|record| record.owner.is_some())
             .ok_or(ClipboardSelectionExecutionError::StaleOwnerGeneration)?;
         if owner_record.generation != grant.source_generation {
             return Err(ClipboardSelectionExecutionError::StaleOwnerGeneration);
         }
         let owner = owner_record
             .owner
             .ok_or(ClipboardSelectionExecutionError::StaleOwnerGeneration)?;
         let raw = 0x0001_0000u32
             .checked_add(self.next_clipboard_proxy)
             .filter(|raw| *raw < 0x0020_0000)
             .ok_or(ClipboardSelectionExecutionError::ExecutorFailure)?;
         self.next_clipboard_proxy = self.next_clipboard_proxy.saturating_add(1);
         let proxy = ClipboardSelectionProxy {
             client: None,
             transfer: grant.transfer,
             namespace: grant.source_namespace,
             owner,
             requestor: crate::XResourceId::new(u64::from(raw), 1),
             selection,
             target,
             property: target,
             time,
         };
         self.clipboard_proxies.insert(proxy.requestor, proxy);
         Ok(proxy)
     }
 
     pub fn is_clipboard_proxy(&self, namespace: NamespaceId, window: crate::XResourceId) -> bool {
         self.clipboard_proxies
             .get(&window)
             .is_some_and(|proxy| proxy.namespace == namespace)
     }

     pub(crate) fn clipboard_proxy(&self, window: crate::XResourceId) -> Option<&ClipboardSelectionProxy> {
         self.clipboard_proxies.get(&window)
     }

     pub(crate) fn bind_clipboard_proxy_client(&mut self, window: crate::XResourceId, client: u64) {
         if let Some(proxy) = self.clipboard_proxies.get_mut(&window) {
             proxy.client = Some(client);
         }
     }
 
     pub fn capture_clipboard_source_payload(
         &mut self,
         requestor: crate::XResourceId,
         property: crate::XAtom,
         properties: &mut XPropertyTable,
     ) -> Result<ClipboardSourcePayload, ClipboardSelectionExecutionError> {
         let proxy = self
             .clipboard_proxies
             .remove(&requestor)
             .ok_or(ClipboardSelectionExecutionError::UnknownTransfer)?;
         if property == X_ATOM_NONE || property != proxy.property {
             properties.remove_window(proxy.namespace, proxy.requestor);
             return Err(ClipboardSelectionExecutionError::ExecutorFailure);
         }
         let record = properties
             .get(proxy.namespace, proxy.requestor, property)
             .cloned();
         properties.remove_window(proxy.namespace, proxy.requestor);
         let record = record.ok_or(ClipboardSelectionExecutionError::MissingProperty)?;
         if record.format != 8 || record.property_type != proxy.target {
             return Err(ClipboardSelectionExecutionError::UnsupportedTarget);
         }
         let bytes = record.bytes;
         if bytes.len() > crate::MAX_CLIPBOARD_TEXT_HANDOFF_BYTES {
             return Err(ClipboardSelectionExecutionError::PayloadTooLarge);
         }
         Ok(ClipboardSourcePayload {
             transfer: proxy.transfer,
             bytes,
         })
     }
 
     pub fn discard_clipboard_proxies(
         &mut self,
         transfer: sophia_protocol::PortalTransferId,
     ) -> Vec<(NamespaceId, crate::XResourceId)> {
         let removed = self
             .clipboard_proxies
             .values()
             .filter(|proxy| proxy.transfer == transfer)
             .map(|proxy| (proxy.namespace, proxy.requestor))
             .collect::<Vec<_>>();
         self.clipboard_proxies
             .retain(|_, proxy| proxy.transfer != transfer);
         removed
     }
 
     /// Completes one broker-approved clipboard transfer. X11 request context
     /// stays in the authority; the executor supplies only a correlated,
     /// bounded payload.
     pub fn execute_clipboard_payload(
         &mut self,
         transfer: sophia_protocol::PortalTransferId,
         grant: &sophia_protocol::PortalGrant,
         payload: &[u8],
         atoms: &mut XAtomTable,
         properties: &mut XPropertyTable,
     ) -> Result<ClipboardSelectionExecutionOutcome, ClipboardSelectionExecutionError> {
         self.execute_clipboard_payload_inner(transfer, grant, payload, atoms, properties, false)
     }

     pub(crate) fn execute_admitted_clipboard_payload(&mut self,
         work: &sophia_portal::files::PortalFileExecution,
         lease: &sophia_portal::coordinator::PortalAccessLease,
         atoms: &mut XAtomTable, properties: &mut XPropertyTable,
     ) -> Result<ClipboardSelectionExecutionOutcome, ClipboardSelectionExecutionError> {
         if lease.grant() != &work.grant {
             return Err(ClipboardSelectionExecutionError::Denied);
         }
         self.execute_clipboard_payload_inner(work.grant.transfer, &work.grant,
             &work.payload, atoms, properties, true)
     }

     fn execute_clipboard_payload_inner(&mut self,
         transfer: sophia_protocol::PortalTransferId, grant: &sophia_protocol::PortalGrant,
         payload: &[u8], atoms: &mut XAtomTable, properties: &mut XPropertyTable,
         admitted: bool,
     ) -> Result<ClipboardSelectionExecutionOutcome, ClipboardSelectionExecutionError> {
         let pending = self
             .pending_clipboard
             .remove(&transfer)
             .ok_or(ClipboardSelectionExecutionError::UnknownTransfer)?;
         let failure = pending.portal_request.failure;
         let fail = |error| ClipboardSelectionExecutionOutcome::Failed {
             error,
             notify: ClipboardSelectionNotify {
                 time: failure.time,
                 requestor: failure.requestor,
                 selection: failure.selection,
                 target: failure.target,
                 property: X_ATOM_NONE,
             },
         };
         if grant.transfer != transfer
             || grant.state != sophia_protocol::PortalGrantState::Active
             || grant.source_generation != pending.portal_request.request.generation
             || grant.source_namespace != pending.portal_request.request.source_namespace
             || grant.target_namespace != pending.portal_request.request.target_namespace
         {
             return Ok(fail(ClipboardSelectionExecutionError::StaleOwnerGeneration));
         }
         if pending.external_source {
             if !admitted || !self.external_clipboard_offers.get(&failure.selection).is_some_and(|offer|
                 offer.source_namespace == grant.source_namespace && offer.generation == grant.source_generation) {
                 return Ok(fail(ClipboardSelectionExecutionError::StaleOwnerGeneration));
             }
         } else {
          let Some(owner) = self
             .selections
             .owner(
                 failure.selection,
                 Some(pending.portal_request.request.source_namespace),
             )
             .filter(|record| record.owner.is_some())
         else {
             return Ok(fail(ClipboardSelectionExecutionError::StaleOwnerGeneration));
         };
         if owner.generation != pending.portal_request.request.generation {
             return Ok(fail(ClipboardSelectionExecutionError::StaleOwnerGeneration));
         }
         }
         if pending.portal_request.property == X_ATOM_NONE {
             return Ok(fail(ClipboardSelectionExecutionError::MissingProperty));
         }
         if payload.len() > crate::MAX_CLIPBOARD_TEXT_HANDOFF_BYTES {
             return Ok(fail(ClipboardSelectionExecutionError::PayloadTooLarge));
         }
         let selection_name = atoms.name(failure.selection);
         if selection_name != Some("PRIMARY") && selection_name != Some("CLIPBOARD") {
             return Ok(fail(ClipboardSelectionExecutionError::UnsupportedTarget));
         }
         let target_name = pending.portal_request.request.target.as_str();
         let (property_type, format, bytes) = match target_name {
             "TARGETS" => {
                 let targets = ["TARGETS", "UTF8_STRING", "text/plain;charset=utf-8"];
                 let mut bytes = Vec::with_capacity(targets.len() * 4);
                 for name in targets {
                     let atom = atoms
                         .intern(name, false)
                         .map_err(|_| ClipboardSelectionExecutionError::Property)?
                         .expect("intern without only-if-exists returns an atom");
                     match pending.byte_order {
                         XByteOrder::LittleEndian => bytes.extend_from_slice(&atom.to_le_bytes()),
                         XByteOrder::BigEndian => bytes.extend_from_slice(&atom.to_be_bytes()),
                     }
                 }
                 (X_ATOM_ATOM, 32, bytes)
             }
             "UTF8_STRING" | "text/plain" | "text/plain;charset=utf-8" => {
                 if core::str::from_utf8(payload).is_err() {
                     return Ok(fail(ClipboardSelectionExecutionError::InvalidUtf8));
                 }
                 (failure.target, 8, payload.to_vec())
             }
             _ => return Ok(fail(ClipboardSelectionExecutionError::UnsupportedTarget)),
         };
         if properties
             .apply_change(
                 pending.namespace,
                 XPropertyChange {
                     mode: XPropertyMode::Replace,
                     window: failure.requestor,
                     property: pending.portal_request.property,
                     property_type,
                     format,
                     bytes: bytes.clone(),
                 },
             )
             .is_err()
         {
             return Ok(fail(ClipboardSelectionExecutionError::Property));
         }
         Ok(ClipboardSelectionExecutionOutcome::Handoff(
             ClipboardSelectionHandoff {
                 transfer,
                 property: ClipboardTextProperty {
                     requestor: failure.requestor,
                     property: pending.portal_request.property,
                     target: failure.target,
                     bytes,
                 },
                 notify: ClipboardSelectionNotify {
                     time: failure.time,
                     requestor: failure.requestor,
                     selection: failure.selection,
                     target: failure.target,
                     property: pending.portal_request.property,
                 },
             },
         ))
     }
 
     pub fn fail_clipboard_transfer(
         &mut self,
         transfer: sophia_protocol::PortalTransferId,
         error: ClipboardSelectionExecutionError,
     ) -> Result<ClipboardSelectionExecutionOutcome, ClipboardSelectionExecutionError> {
         let pending = self
             .pending_clipboard
             .remove(&transfer)
             .ok_or(ClipboardSelectionExecutionError::UnknownTransfer)?;
         let request = pending.portal_request.failure;
         Ok(ClipboardSelectionExecutionOutcome::Failed {
             error,
             notify: ClipboardSelectionNotify {
                 time: request.time,
                 requestor: request.requestor,
                 selection: request.selection,
                 target: request.target,
                 property: X_ATOM_NONE,
             },
         })
     }
 
     pub fn execute_clipboard_payload_frame(
         &mut self,
         frame: &[u8],
         grant: &sophia_protocol::PortalGrant,
         atoms: &mut XAtomTable,
         properties: &mut XPropertyTable,
     ) -> Result<ClipboardSelectionExecutionOutcome, ClipboardSelectionExecutionError> {
         let (transfer, payload) = sophia_protocol::decode_portal_clipboard_payload_frame(frame)
             .map_err(|_| ClipboardSelectionExecutionError::ExecutorFailure)?;
         self.execute_clipboard_payload(transfer, grant, &payload, atoms, properties)
     }
 
}
