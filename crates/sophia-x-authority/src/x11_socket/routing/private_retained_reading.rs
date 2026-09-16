// What a retained connection IS, read without changing it.
//
// Split by subject from the continuation that holds the work. That file answers
// what one connection's ordered output is and what happens to it; this answers
// what can be SAID about one, which is a different job with a different rule:
// nothing here advances, answers or disposes of anything, and every fact in it
// was established somewhere else.

/// Why a retained connection's wire is where it is.
///
/// FOUR DIFFERENT FACTS, and they are not degrees of one. Having no way to end
/// a wire is not a shutdown that refused; a shutdown that refused is not one
/// that has not been tried; and none of them is an ending. Reporting them as
/// one number would make an operator read "not ended" and go looking for a
/// syscall that never happened.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by reporting that is not attached yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PrivateRetainedEnding {
    /// The wire was ended, and the ending was established.
    Ended,
    /// There is nothing here able to end it. A receiver alone carries no
    /// handle on the connection, so this record cannot end that wire and
    /// cannot say anything about its state either.
    NoCapability,
    /// Ending it was tried and refused, with the kind that refused.
    Refused(std::io::ErrorKind),
    /// It has an ending capability and has not used it yet.
    Unattempted,
}

/// What one retained connection is, as far as anything can establish.
///
/// A READING, NOT A DECISION. Every field is a fact something already
/// established -- a syscall, a channel, a close -- and nothing here advances,
/// answers or disposes of anything. What it is for is the case this whole
/// mechanism keeps producing: a record that cannot finish, which must then be
/// legible enough that its reasons can be told apart without guessing.
///
/// THE REASONS COEXIST. A connection can have no way to end its wire AND an
/// unconfirmed closure; these are independent facts of one bounded record, not
/// alternatives, and reporting either alone would lose the other.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by reporting that is not attached yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PrivateRetainedDisposition {
    /// What closing this connection's endpoint established, if anything.
    closure: Option<PrivateHandoverFence>,
    /// Where its wire stands.
    ending: PrivateRetainedEnding,
    /// Whether its producers are gone, established by the channel finishing
    /// rather than by its being quiet.
    drained: bool,
    /// How many capsules this record is holding for it.
    retained: usize,
    /// Whether this record has spent every close attempt it may make here.
    ///
    /// EXPLICIT, because the difference matters to whoever reads it. A close
    /// that refused and has attempts left will be tried again by the next
    /// visit; one that has none left will not, and no amount of driving will
    /// change it. Reporting both as "refused" leaves a reader waiting for a
    /// retry that is never coming.
    ///
    /// It bounds effort HERE and nothing else: it does not establish
    /// termination, does not finish the close, and does not say what anyone
    /// else may still do about this connection.
    retries_exhausted: bool,
    /// Whether all of the above amounts to a connection that owes nothing.
    settled: bool,
}

#[cfg(unix)]
impl PrivateOrderedContinuation {
    /// Read this record, without changing or concluding anything.
    ///
    /// Everything here was established by something else: a close, a syscall,
    /// a channel that finished. This only puts them side by side, because the
    /// reasons a record cannot finish are independent and an operator looking
    /// at one of them needs to see the others.
    #[cfg_attr(not(test), allow(dead_code))] // Read by reporting that is not attached yet.
    fn disposition(&self) -> PrivateRetainedDisposition {
        let (closure, ending, drained, retained, retries_exhausted) = match self {
            Self::Setup {
                accepted,
                fence,
                ending_refused,
                ended,
                drained,
                retained,
                refusal: _,
            } => (
                *fence,
                match (ended, ending_refused, accepted) {
                    (true, _, _) => PrivateRetainedEnding::Ended,
                    (false, Some(kind), _) => PrivateRetainedEnding::Refused(*kind),
                    (false, None, PrivateOrderedSetupCustody::Receiver(_)) => {
                        PrivateRetainedEnding::NoCapability
                    }
                    (false, None, PrivateOrderedSetupCustody::Transport(_)) => {
                        PrivateRetainedEnding::Unattempted
                    }
                },
                *drained,
                retained.len(),
                // A setup record has no close of its own to spend attempts on.
                false,
            ),
            Self::Serving { owner, fence } => (
                *fence,
                // AN ESTABLISHED ENDING IS THE ENDING, whatever else is
                // recorded beside it. More than one path ends a wire and they
                // do not all leave a close record -- ending a part-written
                // frame writes none -- and a close that succeeded after an
                // earlier attempt refused leaves that earlier cause standing.
                // Reading either of those first reported a wire that had been
                // ended as never attempted, or as refused because it once was.
                //
                // The cause is not discharged by being outranked here. It is
                // still on the owner, still says what happened, and this
                // report saying Ended authorises nothing about it.
                if owner.ending_ended() {
                    PrivateRetainedEnding::Ended
                } else {
                    match (owner.closing(), owner.unterminated_cause()) {
                        (Some(closing), _)
                            if matches!(
                                closing.termination,
                                X11OrderedTermination::Refused(_)
                            ) =>
                        {
                            let X11OrderedTermination::Refused(kind) = closing.termination
                            else {
                                unreachable!("matched above")
                            };
                            PrivateRetainedEnding::Refused(kind)
                        }
                        (_, Some(X11OrderedUnterminatedCause::Shutdown(kind))) => {
                            PrivateRetainedEnding::Refused(kind)
                        }
                        _ => PrivateRetainedEnding::Unattempted,
                    }
                },
                owner
                    .closing()
                    .is_some_and(|closing| closing.drained),
                // EVERY SLOT THAT IS HOLDING SOMETHING. A capsule mid-flight
                // and one classified as another endpoint's are as much this
                // owner's custody as the unanswered ones, and a count that
                // left them out reported nothing held while an unanswered
                // completion sat in the writer. What is NOT counted is the
                // queue: asking a channel how much is in it means receiving
                // from it, and a reading may not consume what it reports on.
                owner.retained_unanswered().len()
                    + owner.retained_foreign().len()
                    + usize::from(owner.in_flight().is_some())
                    + usize::from(owner.refused().is_some()),
                owner.closing().is_some_and(|closing| {
                    closing.termination != X11OrderedTermination::Established
                        && closing.attempts >= X11_ORDERED_CLOSE_ATTEMPTS
                }),
            ),
        };
        PrivateRetainedDisposition {
            closure,
            ending,
            drained,
            retained,
            retries_exhausted,
            settled: self.settled(),
        }
    }
}

#[cfg(unix)]
impl PrivateSettlementOwner {
    /// Read every retained connection this store is holding.
    ///
    /// ONE READING PER PLACE THAT HOLDS SOMETHING. Free places and places that
    /// were reserved and never filled are not connections and are not
    /// reported.
    ///
    /// THE INDEX IS SNAPSHOT-LOCAL. It says where a record sat in this
    /// reading, and nothing more: a place that comes back is reused, so the
    /// same index in a later reading may be a different connection entirely.
    /// Anything correlating two readings needs a witness carried from the
    /// record itself, which this does not provide.
    ///
    /// A record that cannot be read is reported as unreadable rather than
    /// skipped or guessed at -- `None` in its row. Silently treating a
    /// poisoned record as a normal one would put an account in front of a
    /// reader with nothing standing behind it, and skipping it would lose a
    /// connection from the account altogether. This is a reading: it is not
    /// permission to drop such a record or to write over it.
    ///
    /// What a store owes in CAPACITY is a different account -- reserved,
    /// retained, abandoned -- and stays separate, because a place held for a
    /// connection nobody can read is still a place held.
    ///
    /// Nothing is driven, received, ended or disposed of here. A store that
    /// cannot be read at all reports nothing rather than an empty account.
    #[cfg_attr(not(test), allow(dead_code))] // Read by reporting that is not attached yet.
    fn retained_dispositions(
        &self,
    ) -> Option<Vec<(usize, Option<PrivateRetainedDisposition>)>> {
        // The aggregate lock finds the records; each record's own lock reads
        // it. Holding the aggregate across those would put the whole store
        // behind one connection.
        let places: Vec<(usize, Arc<Mutex<Option<PrivateOrderedContinuation>>>)> = {
            let held = self.inner.lock().ok()?;
            held.continuations
                .iter()
                .enumerate()
                .filter_map(|(index, place)| match place {
                    PrivateOrderedContinuationPlace::Taken(record) => {
                        Some((index, record.clone()))
                    }
                    PrivateOrderedContinuationPlace::Free => None,
                })
                .collect()
        };
        Some(
            places
                .into_iter()
                .filter_map(|(index, record)| match record.lock() {
                    Ok(held) => held
                        .as_ref()
                        .map(|continuation| (index, Some(continuation.disposition()))),
                    // Held by something that panicked. There may well be a
                    // connection here; what there is not is a reading of it.
                    Err(_) => Some((index, None)),
                })
                .collect(),
        )
    }
}
