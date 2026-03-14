#![no_std]

use crate::collab::CollabSession;
use crate::nexus::NodeId;
use crate::qaudit_kernel::QAuditKernel;

/// Bridge for Phase 297: Collab Session Peer Join Audit Bridge
/// Fires an audit event (Law 4: Global Data State) whenever a new distributed mesh peer joins an active CRDT session.
pub struct CollabSessionJoinAuditBridge<'a> {
    target: &'a mut CollabSession,
}

impl<'a> CollabSessionJoinAuditBridge<'a> {
    pub fn new(target: &'a mut CollabSession) -> Self {
        Self { target }
    }

    pub fn peer_join(
        &mut self,
        peer: NodeId,
        audit: &mut QAuditKernel,
        tick: u64,
    ) {
        audit.log_law_violation(
            4, // Law 4: Global Data State
            self.target.owner_silo,
            tick,
        );
        crate::serial_println!(
            "[COLLAB AUDIT] Peer {:08x} joined session {}. Audit logged.", 
            peer.short_hex(), self.target.session_id
        );

        self.target.peer_join(peer)
    }
}
