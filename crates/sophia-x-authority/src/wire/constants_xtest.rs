// XTEST's constants, beside the core ones rather than among them: the
// core file is at the layout limit and this extension is a self-contained
// vocabulary, so it reads better whole than interleaved.

/// XTEST, the synthetic-input adapter.
///
/// The extension exists only inside an explicitly admitted private instance.
/// Discovery and every request path share one admission decision, so a client
/// that was not admitted finds XTEST absent from QueryExtension and
/// ListExtensions and meets `BadAccess` on a guessed opcode rather than the
/// `BadRequest` an undecoded major would give. Advertising is deliberately the
/// last step of the milestone: a half-honoured XTEST is worse than none, since
/// a client that finds the extension present assumes injection works.
pub const X_TEST_EXTENSION_NAME: &str = "XTEST";
/// 146, the next free major. The assigned range runs 130 through 145.
///
/// XLibre reserves 160 for XTEST by name, and upstream X.Org assigns whatever
/// registration order yields. Neither matters to a client, which learns the
/// major from QueryExtension; it matters only to a test that hardcodes one,
/// and the conformance profile hardcodes this.
pub const X_TEST_MAJOR_OPCODE: u8 = 146;
/// XTEST defines no events and no errors, so QueryExtension answers zero for
/// both bases and the extension raises core errors only.
pub const X_TEST_FIRST_EVENT: u8 = 0;
pub const X_TEST_FIRST_ERROR: u8 = 0;

pub const X_TEST_GET_VERSION_MINOR_OPCODE: u8 = 0;
pub const X_TEST_COMPARE_CURSOR_MINOR_OPCODE: u8 = 1;
pub const X_TEST_FAKE_INPUT_MINOR_OPCODE: u8 = 2;
pub const X_TEST_GRAB_CONTROL_MINOR_OPCODE: u8 = 3;
pub const X_TEST_LAST_MINOR_OPCODE: u8 = X_TEST_GRAB_CONTROL_MINOR_OPCODE;

/// The version answered to every request, whatever was asked for.
///
/// This is not a negotiation and no per-client version state is kept. The
/// reference server never reads the requested version at all and replies with
/// its own constant; we answer 2.1 because 2.2's only wire difference is that
/// byte 35 of FakeInput becomes an XInput device selector, and XI fake input
/// is refused here. A client that sends 2.2's selector anyway is tolerated:
/// for the core event types that byte is padding and is never read.
pub const X_TEST_MAJOR_VERSION: u16 = 2;
pub const X_TEST_MINOR_VERSION: u16 = 1;

/// `CurrentCursor`, the cursor the pointer is displaying.
///
/// Intercepted before any resource lookup, so a real cursor whose id happened
/// to be 1 would be unreachable through CompareCursor. That is the reference
/// behaviour and clients depend on it.
pub const X_TEST_CURRENT_CURSOR: u32 = 1;

/// FakeInput's motion detail: absolute or relative, and nothing else.
///
/// Strictly these two values. Two is `BadValue`, not a second spelling of
/// relative.
pub const X_TEST_MOTION_ABSOLUTE: u8 = 0;
pub const X_TEST_MOTION_RELATIVE: u8 = 1;

/// The send-event bit, masked off a FakeInput type and otherwise ignored.
///
/// The reference server reads the type as `type & 0177`, so a client that
/// sets the high bit gets an ordinary event rather than a refusal. An error
/// still reports the byte that arrived, which is why the unmasked value is
/// carried alongside the masked one.
pub const X_TEST_EVENT_TYPE_MASK: u8 = 0x7f;

/// The virtual device an XTEST connection injects through.
///
/// Names a device within this connection's own grant, not a shared one and
/// not a physical one: the issuer allocates it per grant, and grants are per
/// connection, so every injector gets its own however many there are. A core
/// FakeInput carries no way to name a device anyway -- its trailing 2.1 byte
/// is padding here -- so there is nothing for a client to choose between.
pub const X_TEST_INJECTION_DEVICE: sophia_protocol::DeviceId =
    sophia_protocol::DeviceId::from_raw(1);

/// The surface the pointer is on when it is on the bare root.
///
/// The root is no client's window and Engine commits no surface for it, so
/// nothing in the registry can stand for "the pointer is over the root". This
/// names that state without pretending a surface exists: generation zero is
/// never issued, so it collides with nothing and routes nowhere. A pointer
/// query over it answers root coordinates and no child, which is the right
/// answer; a route to it is refused, which is also right, since there is no
/// client to receive it.
pub const ROOT_POINTER_SURFACE: sophia_protocol::SurfaceId = sophia_protocol::SurfaceId::new(0, 0);

const X_TEST_GET_VERSION_REQ_LEN: usize = 8;
const X_TEST_COMPARE_CURSOR_REQ_LEN: usize = 12;
/// The 36-byte request: a four-byte header and one 32-byte event-shaped body.
const X_TEST_FAKE_INPUT_REQ_LEN: usize = 36;
/// One event record. The body is a whole number of these and at least one;
/// more than one is only ever meaningful to the XInput path, which is refused
/// here, so the count is carried to the dispatcher rather than resolved in the
/// decoder -- a core type with two records is a length fault, and an XInput
/// type with two is a refused extension event.
const X_TEST_FAKE_INPUT_EVENT_LEN: usize = 32;
const X_TEST_FAKE_INPUT_HEADER_LEN: usize =
    X_TEST_FAKE_INPUT_REQ_LEN - X_TEST_FAKE_INPUT_EVENT_LEN;
const X_TEST_GRAB_CONTROL_REQ_LEN: usize = 8;

/// `CompareCursor`'s cursor argument: no cursor at all, not resource zero.
pub const X_TEST_CURSOR_NONE: u32 = 0;
/// `CompareCursor`'s cursor argument: whatever the pointer is showing now.
pub const X_TEST_CURSOR_CURRENT: u32 = 1;
