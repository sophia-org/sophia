mod support;

use std::fs::File;
use std::os::fd::OwnedFd;

use sophia_image_producer::{
    ChannelState, CloseCause, DiscoveryRead, ImagePath, ImageProducer, ImageProducerConfig,
    ImageRole, ProducerError, create_isb,
};
use support::{
    config, discovery_text, formats_body, limits_body, metadata, present, sealed_fd, seqpacket_pair,
};

fn config_revision(role: ImageRole, revision: u16) -> ImageProducerConfig {
    ImageProducerConfig {
        revision,
        ..config(role)
    }
}

#[test]
fn config_accepts_only_known_revisions() {
    assert!(ImageProducer::new(config_revision(ImageRole::Shell, 9), 0).is_ok());
    assert_eq!(
        ImageProducer::new(config_revision(ImageRole::Shell, 8), 0).err(),
        Some(ProducerError::Argument)
    );
    assert!(ImageProducer::new(config_revision(ImageRole::Lock, 2), 0).is_ok());
    assert_eq!(
        ImageProducer::new(config_revision(ImageRole::Lock, 3), 0).err(),
        Some(ProducerError::Argument)
    );
}

#[test]
fn absent_discovery_closes_clean_and_bytes_path_opens() {
    let mut producer = ImageProducer::new(config(ImageRole::Shell), 0).unwrap();
    assert_eq!(
        producer.discover(DiscoveryRead::Absent, 1, 0),
        Err(ProducerError::Closed)
    );
    assert_eq!(producer.channel(), ChannelState::Closed);
    assert_eq!(producer.cause(), CloseCause::Absent);
    assert_eq!(producer.path(), ImagePath::Bytes);
}

#[test]
fn generation_mismatch_is_the_only_retryable_discovery() {
    let text = discovery_text(ImageRole::Shell);
    let wrong = limits_body(ImageRole::Shell, 8);
    let right = limits_body(ImageRole::Shell, 9);
    let formats = formats_body(ImageRole::Shell);
    let mut producer = ImageProducer::new(config(ImageRole::Shell), 0).unwrap();
    assert_eq!(
        producer.discover(present(&text, &wrong, &formats), 1, 0),
        Err(ProducerError::Again)
    );
    assert_eq!(producer.channel(), ChannelState::Idle);
    assert_eq!(
        producer.discover(present(&text, &right, &formats), 1, 1),
        Err(ProducerError::Again)
    );
    assert_eq!(
        producer.discover(present(&text, &right, &formats), 2, 2),
        Ok(())
    );
    assert_eq!(producer.channel(), ChannelState::Discovered);
}

#[test]
fn malformed_discovery_is_fatal_not_retryable() {
    let limits = limits_body(ImageRole::Shell, 9);
    let formats = formats_body(ImageRole::Shell);
    let mut producer = ImageProducer::new(config(ImageRole::Shell), 0).unwrap();
    assert_eq!(
        producer.discover(present(b"nonsense\n", &limits, &formats), 1, 0),
        Err(ProducerError::Closed)
    );
    assert_eq!(producer.cause(), CloseCause::Discovery);
    assert_eq!(producer.path(), ImagePath::Error);
}

#[test]
fn attach_refusal_returns_the_descriptor() {
    let mut producer = ImageProducer::new(config(ImageRole::Shell), 0).unwrap();
    let (mine, _peer) = seqpacket_pair();
    let (error, returned) = producer.attach(mine, 0).unwrap_err();
    assert_eq!(error, ProducerError::Refused);
    drop(returned);
}

#[test]
fn attach_refuses_a_stream_socket_and_returns_it() {
    use std::os::unix::net::UnixStream;
    let text = discovery_text(ImageRole::Shell);
    let limits = limits_body(ImageRole::Shell, 9);
    let formats = formats_body(ImageRole::Shell);
    let mut producer = ImageProducer::new(config(ImageRole::Shell), 0).unwrap();
    producer
        .discover(present(&text, &limits, &formats), 1, 0)
        .unwrap();
    let (mine, _peer) = UnixStream::pair().unwrap();
    let (error, returned) = producer.attach(OwnedFd::from(mine), 1).unwrap_err();
    assert_eq!(error, ProducerError::Argument);
    drop(returned);
    assert_eq!(producer.channel(), ChannelState::Discovered);
}

#[test]
fn begin_refusal_returns_the_source_descriptor() {
    let mut producer = ImageProducer::new(config(ImageRole::Shell), 0).unwrap();
    let (error, returned) = producer.begin(metadata(21), sealed_fd(), 0).unwrap_err();
    assert_eq!(error, ProducerError::Again);
    drop(returned);
    let null: OwnedFd = File::open("/dev/null").unwrap().into();
    let (error, returned) = producer.begin(metadata(21), null, 1).unwrap_err();
    assert_eq!(error, ProducerError::Again);
    drop(returned);
}

#[test]
fn sealed_write_is_bounded_and_exact() {
    let source = create_isb(2, 2).unwrap();
    assert_eq!(source.bytes(), 16);
    // Out-of-bounds writes are refused before any byte moves.
    assert_eq!(
        source.write_pixels(15, &[1u8; 2]),
        Err(ProducerError::Argument)
    );
    assert_eq!(
        source.write_pixels(u64::MAX, &[1u8; 1]),
        Err(ProducerError::Argument)
    );
    let fd = source.into_fd();
    let stat = rustix::fs::fstat(&fd).unwrap();
    assert_eq!(stat.st_size, 16);

    let source = create_isb(2, 2).unwrap();
    source.write_pixels(0, &[1u8; 8]).unwrap();
    source.write_pixels(8, &[2u8; 8]).unwrap();
    let mut back = [0u8; 16];
    assert_eq!(rustix::io::pread(source.fd(), &mut back, 0).unwrap(), 16);
    assert_eq!(back, [1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2]);
}
