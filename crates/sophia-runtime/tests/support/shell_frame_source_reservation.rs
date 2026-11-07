use super::*;
use crate::{ContentCandidateError as Error, ContentFrameAdmission as Admission};

#[test]
fn provisional_source_cancel_preserves_identity_and_releases_both_holds() {
    let mut files = export();
    let mut epochs = epochs();
    let _ring = ready(&mut files, &mut epochs);
    let before = files.owned.last;
    let request = offer(1, 0, 2);
    for _ in 0..16 {
        let held = files.reserve_owned_offer(request).unwrap().unwrap();
        assert_eq!(files.owned.last, before);
        assert_eq!(files.owned.generations[0], 0);
        assert!(files.owned_pending_offer().is_none());
        assert!(matches!(files.reserve_owned_offer(request), Ok(None)));
        files.cancel_owned_offer(held);
        assert!(files.owned.live.iter().all(Option::is_none));
    }
    let held = files.reserve_owned_offer(request).unwrap().unwrap();
    files.commit_owned_offer(held);
    assert_eq!(files.owned.last, Some(Control::Offer(request)));
    assert_eq!(files.owned.generations[0], 2);
    assert!(
        files.owned_pending_offer().is_none(),
        "no synthetic legacy offer"
    );
    assert!(matches!(
        files.reserve_owned_offer(request),
        Err(Errno::EALREADY)
    ));
    let next = files.reserve_owned_offer(offer(2, 1, 2)).unwrap().unwrap();
    files.cancel_owned_offer(next);
}

#[test]
fn second_source_hold_failure_releases_the_first_without_spending_identity() {
    let mut files = export();
    let mut epochs = epochs();
    let _ring = ready(&mut files, &mut epochs);
    files.journal = Journal::new(
        1,
        JournalBounds {
            bytes: 1024,
            reserve_bytes: 100,
        },
        Instant::now(),
    );
    let before = files.owned.last;
    for _ in 0..16 {
        assert!(matches!(
            files.reserve_owned_offer(offer(1, 0, 2)),
            Err(Errno::EAGAIN)
        ));
        assert_eq!(files.owned.last, before);
        assert_eq!(files.owned.generations[0], 0);
        assert!(files.owned.live.iter().all(Option::is_none));
        let held = files
            .journal
            .hold_reservation(88)
            .expect("failed pair returned first hold");
        files.journal.release_reservation(held);
    }
}

#[test]
fn whole_commit_deferral_and_error_restore_exact_scene_allocations() {
    let mut files = export();
    let bytes = golden("commit-copy-retire");
    let (_writer, submit) = stage(&mut files, &bytes);
    files.submit_transaction(&submit, false).unwrap();
    let expected = files.peek_frame_request().unwrap().clone();
    let ShellFrameRecord::Commit(frame) = &expected else {
        panic!()
    };
    let expected_offer = Offer::from(frame.image.unwrap()).request;
    let ShellFrameRecord::Commit(frame) = files.peek_frame_request().unwrap() else {
        panic!()
    };
    let ptrs = (
        frame.candidate.surfaces.as_ptr(),
        frame.candidate.placements.as_ptr(),
        frame.candidate.targets.as_ptr(),
    );
    for error in [false, true] {
        let result = files.with_frame_commit(|input| {
            let frame = input.as_ref().unwrap();
            assert_eq!(frame.image, Some(expected_offer));
            assert_eq!(
                (
                    frame.scene.surfaces.as_ptr(),
                    frame.scene.placements.as_ptr(),
                    frame.scene.targets.as_ptr()
                ),
                ptrs
            );
            if error {
                Err(Error::ClockRegression)
            } else {
                Ok(Admission::Deferred)
            }
        });
        assert_eq!(
            result,
            if error {
                Err(Error::ClockRegression)
            } else {
                Ok(Some(Admission::Deferred))
            }
        );
        assert_eq!(files.peek_frame_request(), Some(&expected));
    }
    assert_eq!(
        files.with_frame_commit(|input| {
            let frame = input.take().unwrap();
            assert_eq!(frame.scene.surfaces.as_ptr(), ptrs.0);
            assert_eq!(frame.scene.placements.as_ptr(), ptrs.1);
            Ok(Admission::Refused(Error::Budget))
        }),
        Ok(Some(Admission::Refused(Error::Budget)))
    );
    assert!(files.peek_frame_request().is_none());
}

#[test]
fn stream_control_is_a_barrier_for_complete_commit_admission() {
    let mut files = export();
    let (_writer, submit) = stage(&mut files, &golden("open"));
    files.submit_transaction(&submit, false).unwrap();
    assert_eq!(
        files.with_frame_commit(|_| panic!("Open is not a scene")),
        Ok(None)
    );
    assert!(matches!(
        files.peek_frame_request(),
        Some(ShellFrameRecord::Open { .. })
    ));
}
