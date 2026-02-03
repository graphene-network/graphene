//! Worker lifecycle state machine.
//!
//! Implements the worker state machine as specified in Whitepaper Section 12.4.
//! The state machine tracks worker registration, job capacity, graceful shutdown,
//! and offline detection.
//!
//! ## State Diagram
//!
//! ```text
//! UNREGISTERED → REGISTERED → ONLINE ⟷ BUSY
//!                               ↓
//!                           DRAINING
//!                               ↓
//!                           UNBONDING
//!                               ↓
//!                            EXITED
//!
//! ONLINE/BUSY ⟷ OFFLINE (connection loss/reconnect)
//! ```

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::p2p::messages::WorkerLoad;

/// Errors that can occur during state transitions.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum StateError {
    /// Invalid state transition attempted.
    #[error("Invalid transition from {from:?} via {event:?}")]
    InvalidTransition {
        from: WorkerState,
        event: WorkerEvent,
    },

    /// No slots available for job.
    #[error("No slots available (max: {max}, active: {active})")]
    NoSlotsAvailable { max: u32, active: u32 },

    /// Cannot accept jobs in current state.
    #[error("Cannot accept jobs in state {0:?}")]
    CannotAcceptJobs(WorkerState),
}

/// Worker lifecycle states.
///
/// These states track the worker's position in its lifecycle, from initial
/// registration through active operation to graceful shutdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[repr(u8)]
pub enum WorkerState {
    /// Initial state before Solana registration.
    #[default]
    Unregistered = 0,

    /// Stake confirmed on Solana, awaiting P2P gossip join.
    Registered = 1,

    /// Active and accepting jobs (has available slots).
    Online = 2,

    /// Active but at capacity (no available slots).
    Busy = 3,

    /// Graceful shutdown initiated, finishing current jobs.
    Draining = 4,

    /// Temporarily disconnected from P2P network.
    Offline = 5,

    /// Unbonding period active (14-day cooldown).
    Unbonding = 6,

    /// Terminal state, worker has exited.
    Exited = 7,
}

impl WorkerState {
    /// Returns true if the worker can accept new jobs.
    pub fn can_accept_jobs(&self) -> bool {
        matches!(self, WorkerState::Online)
    }

    /// Returns true if the worker is in an active state (processing or can process).
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            WorkerState::Online | WorkerState::Busy | WorkerState::Draining
        )
    }

    /// Returns true if the worker is in a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, WorkerState::Exited)
    }
}

impl std::fmt::Display for WorkerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkerState::Unregistered => write!(f, "unregistered"),
            WorkerState::Registered => write!(f, "registered"),
            WorkerState::Online => write!(f, "online"),
            WorkerState::Busy => write!(f, "busy"),
            WorkerState::Draining => write!(f, "draining"),
            WorkerState::Offline => write!(f, "offline"),
            WorkerState::Unbonding => write!(f, "unbonding"),
            WorkerState::Exited => write!(f, "exited"),
        }
    }
}

/// Events that trigger state transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkerEvent {
    /// Stake confirmed on Solana (Unregistered → Registered).
    StakeConfirmed,

    /// Successfully joined P2P gossip network (Registered → Online).
    JoinedGossip,

    /// All job slots are now occupied (Online → Busy).
    SlotsFull,

    /// A job slot became available (Busy → Online).
    SlotAvailable,

    /// Graceful shutdown requested (Online/Busy → Draining).
    ShutdownRequested,

    /// All active jobs completed during drain (Draining → Unbonding).
    AllJobsComplete,

    /// Unbonding period complete (Unbonding → Exited).
    UnbondingComplete,

    /// Lost connection to P2P network (Online/Busy → Offline).
    ConnectionLost,

    /// Reconnected to P2P network (Offline → Online).
    Reconnected,
}

/// Worker state machine with atomic slot management.
///
/// Thread-safe state machine that manages worker lifecycle transitions
/// and job slot reservations using atomic operations.
pub struct WorkerStateMachine {
    /// Current state (behind RwLock for transition atomicity).
    state: RwLock<WorkerState>,

    /// Number of available job slots.
    available_slots: AtomicU32,

    /// Maximum number of job slots.
    max_slots: u32,

    /// State before going offline (for reconnection).
    pre_offline_state: RwLock<Option<WorkerState>>,
}

impl WorkerStateMachine {
    /// Create a new state machine with the given slot capacity.
    ///
    /// Starts in `Unregistered` state with all slots available.
    pub fn new(max_slots: u32) -> Self {
        Self {
            state: RwLock::new(WorkerState::Unregistered),
            available_slots: AtomicU32::new(max_slots),
            max_slots,
            pre_offline_state: RwLock::new(None),
        }
    }

    /// Create a new state machine wrapped in Arc for sharing.
    pub fn new_shared(max_slots: u32) -> Arc<Self> {
        Arc::new(Self::new(max_slots))
    }

    /// Get the current state.
    pub fn state(&self) -> WorkerState {
        *self.state.read().unwrap()
    }

    /// Get the number of available slots.
    pub fn available_slots(&self) -> u32 {
        self.available_slots.load(Ordering::SeqCst)
    }

    /// Get the maximum number of slots.
    pub fn max_slots(&self) -> u32 {
        self.max_slots
    }

    /// Get the number of active (occupied) slots.
    pub fn active_slots(&self) -> u32 {
        self.max_slots - self.available_slots()
    }

    /// Get the current load for gossip messages.
    pub fn load(&self) -> WorkerLoad {
        WorkerLoad {
            available_slots: self.available_slots().min(255) as u8,
            queue_depth: 0, // TODO(#44): Track queue depth when job queue is implemented
        }
    }

    /// Check if the worker can accept a new job.
    ///
    /// Returns true only when in `Online` state with available slots.
    pub fn can_accept_job(&self) -> bool {
        let state = self.state();
        state.can_accept_jobs() && self.available_slots() > 0
    }

    /// Execute a state transition.
    ///
    /// Returns the new state on success, or an error if the transition is invalid.
    pub fn transition(&self, event: WorkerEvent) -> Result<WorkerState, StateError> {
        let mut state = self.state.write().unwrap();
        let current = *state;

        let next = self.next_state(current, event)?;

        // Handle special cases for offline transitions
        if event == WorkerEvent::ConnectionLost {
            let mut pre_offline = self.pre_offline_state.write().unwrap();
            *pre_offline = Some(current);
        }

        *state = next;
        Ok(next)
    }

    /// Compute the next state for a given transition.
    fn next_state(
        &self,
        current: WorkerState,
        event: WorkerEvent,
    ) -> Result<WorkerState, StateError> {
        use WorkerEvent::*;
        use WorkerState::*;

        let next = match (current, event) {
            // Registration flow
            (Unregistered, StakeConfirmed) => Registered,
            (Registered, JoinedGossip) => Online,

            // Capacity transitions
            (Online, SlotsFull) => Busy,
            (Busy, SlotAvailable) => Online,

            // Shutdown flow
            (Online, ShutdownRequested) | (Busy, ShutdownRequested) => Draining,
            (Draining, AllJobsComplete) => Unbonding,
            (Unbonding, UnbondingComplete) => Exited,

            // Connectivity transitions
            (Online, ConnectionLost) | (Busy, ConnectionLost) => Offline,
            (Offline, Reconnected) => {
                // Return to pre-offline state, defaulting to Online
                let pre_offline = self.pre_offline_state.read().unwrap();
                match *pre_offline {
                    Some(Busy) => Busy,
                    _ => Online,
                }
            }

            // Invalid transition
            _ => {
                return Err(StateError::InvalidTransition {
                    from: current,
                    event,
                })
            }
        };

        Ok(next)
    }

    /// Try to reserve a job slot, returning a guard on success.
    ///
    /// The guard will automatically release the slot when dropped.
    /// This may trigger a state transition to `Busy` if this was the last slot.
    pub fn try_reserve_slot(self: &Arc<Self>) -> Result<SlotGuard, StateError> {
        let state = self.state();

        // Check if we can accept jobs
        if !state.can_accept_jobs() {
            return Err(StateError::CannotAcceptJobs(state));
        }

        // Try to decrement available slots atomically
        loop {
            let current = self.available_slots.load(Ordering::SeqCst);
            if current == 0 {
                return Err(StateError::NoSlotsAvailable {
                    max: self.max_slots,
                    active: self.max_slots,
                });
            }

            if self
                .available_slots
                .compare_exchange(current, current - 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                // Transition to Busy if this was the last slot
                if current == 1 {
                    let _ = self.transition(WorkerEvent::SlotsFull);
                }

                return Ok(SlotGuard {
                    machine: Arc::clone(self),
                });
            }
        }
    }

    /// Release a slot (called internally by SlotGuard::drop).
    fn release_slot(&self) {
        let prev = self.available_slots.fetch_add(1, Ordering::SeqCst);

        // Transition to Online if we were Busy and now have a slot
        if prev == 0 {
            let state = self.state();
            if state == WorkerState::Busy {
                let _ = self.transition(WorkerEvent::SlotAvailable);
            }
        }
    }
}

/// RAII guard for a reserved job slot.
///
/// When dropped, automatically releases the slot and may trigger
/// a state transition from `Busy` to `Online`.
pub struct SlotGuard {
    machine: Arc<WorkerStateMachine>,
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        self.machine.release_slot();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let machine = WorkerStateMachine::new(4);
        assert_eq!(machine.state(), WorkerState::Unregistered);
        assert_eq!(machine.available_slots(), 4);
        assert_eq!(machine.max_slots(), 4);
    }

    #[test]
    fn test_registration_flow() {
        let machine = WorkerStateMachine::new(4);

        // Unregistered → Registered
        let state = machine.transition(WorkerEvent::StakeConfirmed).unwrap();
        assert_eq!(state, WorkerState::Registered);

        // Registered → Online
        let state = machine.transition(WorkerEvent::JoinedGossip).unwrap();
        assert_eq!(state, WorkerState::Online);
    }

    #[test]
    fn test_invalid_transition() {
        let machine = WorkerStateMachine::new(4);

        // Cannot go directly from Unregistered to Online
        let result = machine.transition(WorkerEvent::JoinedGossip);
        assert!(matches!(result, Err(StateError::InvalidTransition { .. })));

        // Cannot go from Unregistered to Busy
        let result = machine.transition(WorkerEvent::SlotsFull);
        assert!(matches!(result, Err(StateError::InvalidTransition { .. })));
    }

    #[test]
    fn test_slot_reservation() {
        let machine = WorkerStateMachine::new_shared(2);

        // Get to Online state
        machine.transition(WorkerEvent::StakeConfirmed).unwrap();
        machine.transition(WorkerEvent::JoinedGossip).unwrap();
        assert_eq!(machine.state(), WorkerState::Online);

        // Reserve first slot
        let _guard1 = machine.try_reserve_slot().unwrap();
        assert_eq!(machine.available_slots(), 1);
        assert_eq!(machine.state(), WorkerState::Online);

        // Reserve second slot (should transition to Busy)
        let _guard2 = machine.try_reserve_slot().unwrap();
        assert_eq!(machine.available_slots(), 0);
        assert_eq!(machine.state(), WorkerState::Busy);

        // Cannot reserve more slots
        let result = machine.try_reserve_slot();
        assert!(matches!(result, Err(StateError::CannotAcceptJobs(_))));
    }

    #[test]
    fn test_slot_guard_drop() {
        let machine = WorkerStateMachine::new_shared(1);

        // Get to Online state
        machine.transition(WorkerEvent::StakeConfirmed).unwrap();
        machine.transition(WorkerEvent::JoinedGossip).unwrap();

        // Reserve the only slot (should go Busy)
        {
            let _guard = machine.try_reserve_slot().unwrap();
            assert_eq!(machine.state(), WorkerState::Busy);
            assert_eq!(machine.available_slots(), 0);
        }
        // Guard dropped, should be back to Online
        assert_eq!(machine.state(), WorkerState::Online);
        assert_eq!(machine.available_slots(), 1);
    }

    #[test]
    fn test_shutdown_flow() {
        let machine = WorkerStateMachine::new(4);

        // Get to Online state
        machine.transition(WorkerEvent::StakeConfirmed).unwrap();
        machine.transition(WorkerEvent::JoinedGossip).unwrap();
        assert_eq!(machine.state(), WorkerState::Online);

        // Request shutdown
        let state = machine.transition(WorkerEvent::ShutdownRequested).unwrap();
        assert_eq!(state, WorkerState::Draining);

        // All jobs complete
        let state = machine.transition(WorkerEvent::AllJobsComplete).unwrap();
        assert_eq!(state, WorkerState::Unbonding);

        // Unbonding complete
        let state = machine.transition(WorkerEvent::UnbondingComplete).unwrap();
        assert_eq!(state, WorkerState::Exited);
        assert!(machine.state().is_terminal());
    }

    #[test]
    fn test_offline_reconnect() {
        let machine = WorkerStateMachine::new(4);

        // Get to Online state
        machine.transition(WorkerEvent::StakeConfirmed).unwrap();
        machine.transition(WorkerEvent::JoinedGossip).unwrap();
        assert_eq!(machine.state(), WorkerState::Online);

        // Go offline
        let state = machine.transition(WorkerEvent::ConnectionLost).unwrap();
        assert_eq!(state, WorkerState::Offline);

        // Reconnect
        let state = machine.transition(WorkerEvent::Reconnected).unwrap();
        assert_eq!(state, WorkerState::Online);
    }

    #[test]
    fn test_offline_from_busy_reconnects_to_busy() {
        let machine = WorkerStateMachine::new_shared(1);

        // Get to Busy state
        machine.transition(WorkerEvent::StakeConfirmed).unwrap();
        machine.transition(WorkerEvent::JoinedGossip).unwrap();
        let _guard = machine.try_reserve_slot().unwrap();
        assert_eq!(machine.state(), WorkerState::Busy);

        // Go offline
        let state = machine.transition(WorkerEvent::ConnectionLost).unwrap();
        assert_eq!(state, WorkerState::Offline);

        // Reconnect should return to Busy (we still have active jobs)
        let state = machine.transition(WorkerEvent::Reconnected).unwrap();
        assert_eq!(state, WorkerState::Busy);
    }

    #[test]
    fn test_can_accept_job() {
        let machine = WorkerStateMachine::new_shared(1);

        // Cannot accept in Unregistered
        assert!(!machine.can_accept_job());

        machine.transition(WorkerEvent::StakeConfirmed).unwrap();
        // Cannot accept in Registered
        assert!(!machine.can_accept_job());

        machine.transition(WorkerEvent::JoinedGossip).unwrap();
        // Can accept in Online with slots
        assert!(machine.can_accept_job());

        let _guard = machine.try_reserve_slot().unwrap();
        // Cannot accept in Busy (no slots)
        assert!(!machine.can_accept_job());
    }

    #[test]
    fn test_concurrent_slot_operations() {
        use std::thread;

        let machine = WorkerStateMachine::new_shared(20);

        // Get to Online state
        machine.transition(WorkerEvent::StakeConfirmed).unwrap();
        machine.transition(WorkerEvent::JoinedGossip).unwrap();

        let handles: Vec<_> = (0..20)
            .map(|_| {
                let m = Arc::clone(&machine);
                thread::spawn(move || m.try_reserve_slot())
            })
            .collect();

        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        // All 20 should succeed
        let successes = results.iter().filter(|r| r.is_ok()).count();
        assert_eq!(successes, 20);

        // No more slots available
        assert_eq!(machine.available_slots(), 0);
        assert_eq!(machine.state(), WorkerState::Busy);
    }

    #[test]
    fn test_load() {
        let machine = WorkerStateMachine::new_shared(4);

        machine.transition(WorkerEvent::StakeConfirmed).unwrap();
        machine.transition(WorkerEvent::JoinedGossip).unwrap();

        let load = machine.load();
        assert_eq!(load.available_slots, 4);
        assert_eq!(load.queue_depth, 0);

        let _guard = machine.try_reserve_slot().unwrap();
        let load = machine.load();
        assert_eq!(load.available_slots, 3);
    }

    #[test]
    fn test_state_display() {
        assert_eq!(format!("{}", WorkerState::Online), "online");
        assert_eq!(format!("{}", WorkerState::Busy), "busy");
        assert_eq!(format!("{}", WorkerState::Draining), "draining");
    }
}
