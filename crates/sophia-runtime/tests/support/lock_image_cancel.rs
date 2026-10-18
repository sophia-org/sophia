//! Lock sealed-image cancellation over the real wire: an unsent
//! offer cancelled by `ResourceCancel`, its duplicate, and channel
//! loss with an unsent offer live. The while-validating cancel lives
//! in-crate (see `lock_files::transport::cancel_tests`), where the
//! owner can dispatch the staged cancel before the worker serves;
//! these controls control every ordering they assert.

use super::*;

impl Files {
    /// Submits `ResourceCancel` naming `offer` and its exact resource;
    /// returns the submit reply's errno, if any.
    fn cancel(&mut self, offer: u64) -> Option<u32> {
        let step = LockResourceStep {
            transaction: offer,
            resource: LockResourceId {
                id: 20 + offer,
                generation: 1,
            },
            total_bytes: None,
        };
        self.submit(LockFileKind::ResourceCancel, &step.encode().unwrap(), 2)
    }

    /// The terminal pair of a cancelled unsent offer: Cancelled with
    /// no reason, released CancelledBeforeImport (4) - the sealed CPU
    /// disposition even for a source already with the worker.
    fn cancelled(&mut self, offer: u64) {
        assert_eq!(self.next().0, LockFileKind::Submitted);
        assert_eq!(
            self.settled(offer),
            (
                LockResourceState::Cancelled,
                0,
                LockSourceDisposition::CancelledBeforeImport,
            )
        );
    }
}

#[test]
fn an_unsent_offer_cancel_settles_once_and_its_duplicate_fails() {
    let (mut transport, directory) = transport("cancel-unsent");
    transport.enable_isb_images(policy(2000)).unwrap();
    let (mut provider, stream) = Provider::start(&mut transport, &directory);
    let provider = pump(
        &mut transport,
        std::thread::spawn(move || {
            let mut files = Files::attach(stream);
            let socket = ready(&mut files, &mut provider);
            files.admitted(1);
            // The cancel settles the unsent offer at once.
            assert_eq!(files.cancel(1), None);
            files.cancelled(1);
            // A duplicate cancel names no live offer: custody falls
            // back to the resource plan, which refuses it with
            // EINVAL, and no second terminal pair follows - the next
            // offer is admitted on the same channel.
            assert_eq!(files.cancel(1), Some(22));
            files.admitted(2);
            drop(socket);
            provider
        }),
    );
    drop(provider);
}

#[test]
fn channel_loss_rejects_an_unsent_offer_after_channel_closed() {
    let (mut transport, directory) = transport("cancel-close");
    transport.enable_isb_images(policy(2000)).unwrap();
    let (mut provider, stream) = Provider::start(&mut transport, &directory);
    let provider = pump(
        &mut transport,
        std::thread::spawn(move || {
            let mut files = Files::attach(stream);
            let socket = ready(&mut files, &mut provider);
            files.admitted(1);
            // Clean close of the image socket: the contract reason is
            // PeerDisconnect, journaled before the unsent offer's
            // rejection with the lock image-unavailable reason (9).
            drop(socket);
            files.closed(LockChannelClosedReason::PeerDisconnect);
            files.unavailable(1);
            provider
        }),
    );
    drop(provider);
}
