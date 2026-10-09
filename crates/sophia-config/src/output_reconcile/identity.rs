use super::*;

/// Bare connector selectors remain convenient on one GPU. A repeated name on
/// multiple admitted GPUs needs an explicit qualifier; enumeration never wins.
pub(super) fn bind_candidate(
    candidate: &DesktopOutputCandidate,
    topology: &DesktopOutputTopologySnapshot,
) -> Result<DesktopOutputCandidate, DesktopOutputReconcileError> {
    let bind = |selector: &str| -> Result<String, DesktopOutputReconcileError> {
        if selector.contains('/') {
            return Ok(selector.into());
        }
        let mut matches = topology
            .connectors
            .iter()
            .filter(|head| head.connector.rsplit('/').next() == Some(selector))
            .map(|head| &head.connector)
            .collect::<BTreeSet<_>>()
            .into_iter();
        let first = matches.next();
        if matches.next().is_some() {
            return Err(DesktopOutputReconcileError::AmbiguousConnector(
                selector.into(),
            ));
        }
        Ok(first.map_or_else(|| selector.to_owned(), |name| name.clone()))
    };
    let mut bound = candidate.clone();
    for named in &mut bound.named {
        named.connector = bind(&named.connector)?;
        for mirrored in &mut named.mirror {
            *mirrored = bind(mirrored)?;
        }
    }
    Ok(bound)
}
