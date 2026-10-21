PREFIX ?= /usr/local
LIBDIR ?= $(PREFIX)/lib
INCLUDEDIR ?= $(PREFIX)/include/sophia-desktop
BUILD ?= build
VERSION = 0.10.0-dev
CC ?= cc
AR ?= ar
CFLAGS ?= -O2 -g
override CFLAGS += -std=c99 -Wall -Wextra -Werror -pedantic
CPPFLAGS += -Isrc

NINE_P = $(sort $(wildcard src/nine_p/*.c))
ADMIN_FILES = $(sort $(wildcard src/admin_files/*.c))
BROKER_FILES = $(sort $(wildcard src/broker_files/*.c))
PORTAL_FILES = $(sort $(wildcard src/portal_files/*.c))
FILES = $(sort $(wildcard src/shell_files/*.c src/shell_session/*.c src/native_session/*.c src/wm_files/*.c src/wm_session/*.c src/output_files/*.c src/output_session/*.c src/lock_files/*.c src/launch_auth/*.c src/namespace/*.c src/image_channel/*.c src/image_producer/*.c)) src/desktop_connection.c
objects = $(patsubst %.c,$(BUILD)/%.o,$(1))
LIBRARIES = $(BUILD)/libsophia-9p.a $(BUILD)/libsophia-desktop.a
PACKAGES = sophia-9p sophia-desktop

.PHONY: all check check-files check-spec check-generator install clean
all: $(LIBRARIES) $(addprefix $(BUILD)/,$(addsuffix .pc,$(PACKAGES)))

.PHONY: check-binding
check-binding: $(LIBRARIES)
	nim c --hints:off --warnings:on --nimcache:$(BUILD)/nim-binding --out:$(BUILD)/requester_binding_smoke --passC:-I$(CURDIR)/src --passL:$(CURDIR)/$(BUILD)/libsophia-desktop.a --passL:$(CURDIR)/$(BUILD)/libsophia-9p.a src/tests/requester_binding_smoke.nim
	$(BUILD)/requester_binding_smoke

$(BUILD)/src/%.o: src/%.c
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) -MMD -MP -c $< -o $@

$(BUILD)/libsophia-9p.a: $(call objects,$(NINE_P)) GNUmakefile
	archive_dir=$$(mktemp -d "$(@D)/archive.XXXXXX") && $(AR) rcs "$$archive_dir/library.a" $(filter %.o,$^) && mv "$$archive_dir/library.a" "$@" && rmdir "$$archive_dir"
$(BUILD)/libsophia-desktop.a: $(call objects,$(FILES) $(ADMIN_FILES) $(PORTAL_FILES) $(BROKER_FILES)) GNUmakefile
	archive_dir=$$(mktemp -d "$(@D)/archive.XXXXXX") && $(AR) rcs "$$archive_dir/library.a" $(filter %.o,$^) && mv "$$archive_dir/library.a" "$@" && rmdir "$$archive_dir"
$(BUILD)/%.pc: pkgconfig/%.pc.in GNUmakefile
	@mkdir -p $(@D)
	sed -e 's|@PREFIX@|$(PREFIX)|g' -e 's|@LIBDIR@|$(LIBDIR)|g' -e 's|@INCLUDEDIR@|$(INCLUDEDIR)|g' -e 's|@VERSION@|$(VERSION)|g' $< > $@

FILE_TESTS = shell_session_attached_test sophia_9p_client_test launch_auth_test sophia_shell_files_test sophia_shell_files_roles_test sophia_shell_files_descriptors_test sophia_shell_files_descriptor_candidates_test sophia_shell_files_descriptor_objects_test sophia_shell_files_staging_test desktop_connection_test shell_session_custody_test shell_session_events_test shell_session_descriptors_test shell_session_staging_test native_session_test wm_files_test wm_session_test output_files_test output_session_test lock_files_test lock_client_test lock_image_test
FILE_TESTS += namespace_test
FILE_TESTS += image_channel_codec_test owned_ring_codec_test
FILE_TESTS += image_receipt_test
FILE_TESTS += image_producer_test
FILE_TESTS += isb_mapped_test
FILE_TESTS += shell_image_test
FILE_TESTS += shell_byte_refusal_test
FILE_TESTS += descriptor_draft_codec_test
FILE_TESTS += broker_discovery_test
FILE_TESTS += broker_events_test
FILE_TESTS += broker_responses_test
FILE_TESTS += broker_controls_test
FILE_TESTS += broker_submission_test
FILE_TESTS += broker_journal_test broker_startup_test broker_transaction_test broker_phase_test broker_session_test
FILE_TESTS += admin_files_test
FILE_TESTS += admin_invocation_test
FILE_TESTS += admin_status_test
FILE_TESTS += admin_catalog_test
FILE_TESTS += admin_snapshot_test
FILE_TESTS += admin_startup_test
FILE_TESTS += admin_command_test
FILE_TESTS += admin_session_test
FILE_TESTS += admin_connecting_test
FILE_TESTS += portal_frame_test
FILE_TESTS += portal_capture_test
FILE_TESTS += portal_discovery_test
FILE_TESTS += portal_snapshot_test
FILE_TESTS += portal_startup_test
FILE_TESTS += portal_read_window_test
FILE_TESTS += portal_frame_client_test
FILE_TESTS += portal_capture_client_test
FILE_TESTS += portal_request_test
FILE_TESTS += portal_transfer_test
FILE_TESTS += portal_watch_test
FILE_TESTS += portal_session_test
FILE_TESTS += portal_session_capture_test
FILE_TESTS += portal_session_frame_test
FILE_TESTS += portal_requester_records_test
FILE_TESTS += portal_requester_state_test
FILE_TESTS += portal_requester_startup_test
FILE_TESTS += portal_requester_transaction_test
FILE_TESTS += portal_requester_session_test
FILE_TESTS += portal_requester_transfer_test
FILE_TESTS += portal_requester_upload_test
ifeq ($(shell uname -s),Linux)
FILE_TESTS += descriptor_kernel_test descriptor_lifecycle_test descriptor_custody_test
$(BUILD)/desktop_connection_test: WRAPS = -Wl,--wrap=getsockopt
endif
$(addprefix $(BUILD)/,$(FILE_TESTS)): $(BUILD)/%: src/tests/%.c $(wildcard src/tests/*.h) $(BUILD)/libsophia-desktop.a $(BUILD)/libsophia-9p.a
	$(CC) $(CPPFLAGS) $(CFLAGS) -UNDEBUG $< -L$(BUILD) -lsophia-desktop -lsophia-9p $(WRAPS) $(LDFLAGS) $(LDLIBS) -o $@
check-files: $(addprefix $(BUILD)/,$(FILE_TESTS))
	@set -e; for test in $^; do "$$test"; done

# Explicit conformance peer: run only under Sophia's protected launch fixture.
$(BUILD)/shell_startup_peer: src/tests/shell_startup_peer.c $(LIBRARIES)
	$(CC) $(CPPFLAGS) $(CFLAGS) -UNDEBUG $< -L$(BUILD) -lsophia-desktop -lsophia-9p $(LDFLAGS) $(LDLIBS) -o $@

$(BUILD)/metadata_peer: src/tests/metadata_peer.c $(LIBRARIES)
	$(CC) $(CPPFLAGS) $(CFLAGS) -UNDEBUG $< -L$(BUILD) -lsophia-desktop -lsophia-9p $(LDFLAGS) $(LDLIBS) -o $@

$(BUILD)/broker_peer: src/tests/broker_peer.c $(LIBRARIES)
	$(CC) $(CPPFLAGS) $(CFLAGS) -UNDEBUG $< -L$(BUILD) -lsophia-desktop -lsophia-9p $(LDFLAGS) $(LDLIBS) -o $@

$(BUILD)/requester_peer: src/tests/requester_peer.c $(LIBRARIES)
	$(CC) $(CPPFLAGS) $(CFLAGS) -UNDEBUG $< -L$(BUILD) -lsophia-desktop -lsophia-9p $(LDFLAGS) $(LDLIBS) -o $@

$(BUILD)/portal_authenticated_peer: src/tests/portal_authenticated_peer.c $(LIBRARIES)
	$(CC) $(CPPFLAGS) $(CFLAGS) -UNDEBUG $< -L$(BUILD) -lsophia-desktop -lsophia-9p $(LDFLAGS) $(LDLIBS) -o $@

$(BUILD)/admin_conformance_peer: src/tests/admin_conformance_peer.c $(LIBRARIES)
	$(CC) $(CPPFLAGS) $(CFLAGS) -UNDEBUG $< -L$(BUILD) -lsophia-desktop -lsophia-9p $(LDFLAGS) $(LDLIBS) -o $@

$(BUILD)/wm_authenticated_peer: src/tests/wm_authenticated_peer.c $(LIBRARIES)
	$(CC) $(CPPFLAGS) $(CFLAGS) -UNDEBUG $< -L$(BUILD) -lsophia-desktop -lsophia-9p $(LDFLAGS) $(LDLIBS) -o $@

$(BUILD)/output_authenticated_peer: src/tests/output_authenticated_peer.c $(LIBRARIES)
	$(CC) $(CPPFLAGS) $(CFLAGS) -UNDEBUG $< -L$(BUILD) -lsophia-desktop -lsophia-9p $(LDFLAGS) $(LDLIBS) -o $@

$(BUILD)/lock_authenticated_peer: src/tests/lock_authenticated_peer.c $(LIBRARIES)
	$(CC) $(CPPFLAGS) $(CFLAGS) -UNDEBUG $< -L$(BUILD) -lsophia-desktop -lsophia-9p $(LDFLAGS) $(LDLIBS) -o $@

check-spec:
	sha256sum --check spec/SHA256SUMS
# Needs python3; not part of check because Python is not a build dependency.
check-generator:
	python3 -B tools/generate_wm_rows.py --check
	cd tools && python3 -B test_generate_wm_rows.py
check: check-spec check-files

install: all
	install -d $(DESTDIR)$(LIBDIR) $(DESTDIR)$(LIBDIR)/pkgconfig $(DESTDIR)$(INCLUDEDIR)
	install -m 644 $(LIBRARIES) $(DESTDIR)$(LIBDIR)/
	install -m 644 src/*.h $(DESTDIR)$(INCLUDEDIR)/
	install -m 644 $(addprefix $(BUILD)/,$(addsuffix .pc,$(PACKAGES))) $(DESTDIR)$(LIBDIR)/pkgconfig/
clean:
	rm -rf $(BUILD)

-include $(patsubst %.o,%.d,$(call objects,$(NINE_P) $(FILES) $(ADMIN_FILES) $(PORTAL_FILES) $(BROKER_FILES)))
