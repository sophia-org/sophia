#!/usr/bin/env python3
"""Build immutable baseline and instrumented copies; never modify Cargo's cache."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tomllib
import clipboard_trace

HERE = Path(__file__).resolve().parent
ARCHIVES = {
    clipboard_trace.ARCHIVE: clipboard_trace.CHECKSUM,
    "winit-0.30.12": "c66d4b9ed69c4009f6321f762d6e61ad8a2389cd431b97cb1e146812e9e6c732",
    "x11rb-0.13.2": "9993aa5be5a26815fe2c3eacfc1fde061fc1a1f094bf1ad2a18bf9c495dd7414",
    "pinentry-egui-0.1.1": "8e94e92457a4f08152a825bb1175bfca805f73af76bdee8d864d37ee7348cf04",
    "eframe-0.33.3": "457481173e6db5ca9fa2be93a58df8f4c7be639587aeb4853b526c6cf87db4e6",
    "egui-winit-0.33.3": "ec6687e5bb551702f4ad10ac428bab12acf9d53047ebb1082d4a0ed8c6251a29",
}

def replace(path, old, new, count=1):
    text = path.read_text()
    if text.count(old) != count:
        raise RuntimeError(f"Source anchor changed: {path.name}: {old[:70]!r}")
    path.write_text(text.replace(old, new))

def mark(stage, value="0"):
    return f't082_trace::mark("{stage}", {value});'

def instrument(root):
    pin = root / "pinentry-egui-0.1.1/src/main.rs"
    replace(pin, "fn main() {", "fn main() {\n    let _trace = t082_trace::start();")
    replace(pin, "fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {",
            "fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {\n" + mark("app_update"))
    replace(pin, "dialog.submitted = Some(true);", mark("submit_enter") + "\n                dialog.submitted = Some(true);", count=2)
    # Distinguish the button from the field without changing either predicate.
    replace(pin, 'if ui.button(ok_text).clicked() {\n                ' + mark("submit_enter"),
            'if ui.button(ok_text).clicked() {\n                ' + mark("submit_ok"))
    replace(pin, "dialog.submitted = Some(false);", mark("submit_cancel") + "\n                dialog.submitted = Some(false);")
    replace(pin, 'if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {',
            'if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {\n' + mark("submit_escape"))
    replace(pin, "let _ = self.tx.send(DialogResult::Cancelled);",
            mark("result_send_enter") + '\nlet sent = self.tx.send(DialogResult::Cancelled);\n' + mark("result_sent", "u64::from(sent.is_ok())"), count=2)
    replace(pin, "let _ = self.tx.send(DialogResult::Confirmed);",
            mark("result_send_enter") + '\nlet sent = self.tx.send(DialogResult::Confirmed);\n' + mark("result_sent", "u64::from(sent.is_ok())"))
    replace(pin, "let _ = self.tx.send(DialogResult::Pin(SecretString::from(",
            mark("result_send_enter") + "\nlet sent = self.tx.send(DialogResult::Pin(SecretString::from(")
    replace(pin, "self.dialog.password.clear();", mark("result_sent", "u64::from(sent.is_ok())") + "\nself.dialog.password.clear();")
    replace(pin, "ctx.send_viewport_cmd(egui::ViewportCommand::Close);",
            mark("close_request_enter") + "\nctx.send_viewport_cmd(egui::ViewportCommand::Close);\n" + mark("close_enqueued"), count=2)
    replace(pin, "if let Err(e) = eframe::run_native(", mark("run_native_enter") + "\nif let Err(e) = eframe::run_native(")
    replace(pin, "Box::new(move |_cc| {", '''Box::new(move |_cc| {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(handle) = _cc.window_handle() {
                let xid = match handle.as_raw() {
                    RawWindowHandle::Xlib(h) => h.window,
                    RawWindowHandle::Xcb(h) => u64::from(h.window.get()),
                    _ => 0,
                };
                t082_trace::mark("window_created", xid);
            }''')
    replace(pin, 'eprintln!("eframe error: {}", e);', 'let _ = e;\n' + mark("run_native_error"))
    replace(pin, 'rx.try_recv().unwrap_or(DialogResult::Cancelled)',
            mark("run_native_return") + '\nlet result = rx.try_recv();\n' + mark("result_received", "u64::from(result.is_ok())") + '\nresult.unwrap_or(DialogResult::Cancelled)')
    replace(pin, 'let current_state = std::mem::take(&mut state);\n                match show_dialog(current_state, true) {',
            mark("getpin_enter") + '\nlet current_state = std::mem::take(&mut state);\n                match show_dialog(current_state, true) {')
    replace(pin, 'respond(&mut stdout, &format!("D {}", encoded));\n                        respond(&mut stdout, "OK");',
            mark("assuan_data_enter") + '\nrespond(&mut stdout, &format!("D {}", encoded));\n' + mark("assuan_data_return") + '\nrespond(&mut stdout, "OK");\n' + mark("assuan_terminal_return", "1"))
    replace(pin, 'respond(&mut stdout, "ERR 83886179 Operation cancelled");',
            'respond(&mut stdout, "ERR 83886179 Operation cancelled");\n' + mark("assuan_terminal_return", "0"), count=2)
    glow = root / "eframe-0.33.3/src/native/glow_integration.rs"
    painting = '''        painter.paint_and_update_textures(
            screen_size_in_pixels,
            pixels_per_point,
            &clipped_primitives,
            &textures_delta,
        );'''
    replace(glow, painting, mark("paint_enter") + "\n" + painting + "\n" + mark("paint_return"))
    replace(glow, "gl_surface.swap_buffers(context)?;",
            mark("swap_enter") + '\nlet swap = gl_surface.swap_buffers(context);\n' + mark("swap_return", "u64::from(swap.is_ok())") + '\nswap?;')
    replace(glow, "glutin.handle_viewport_output(event_loop, &integration.egui_ctx, &viewport_output);",
            mark("viewport_output_enter") + '\nglutin.handle_viewport_output(event_loop, &integration.egui_ctx, &viewport_output);\n' + mark("viewport_output_return"))
    replace(glow, "winit::event::WindowEvent::CloseRequested => {",
            "winit::event::WindowEvent::CloseRequested => {\n" + mark("native_close_requested"))
    epi = root / "eframe-0.33.3/src/native/epi_integration.rs"
    replace(epi, "let close_requested = raw_input.viewport().close_requested();",
            "let close_requested = raw_input.viewport().close_requested();\nif close_requested {" + mark("close_observed") + "}")
    replace(epi, "self.close = true;", "self.close = true;\n" + mark("close_accepted"))
    run = root / "eframe-0.33.3/src/native/run.rs"
    replace(run, "event_loop.exit();", mark("event_loop_exit_requested") + "\nevent_loop.exit();")
    replace(run, "event_loop.run_app_on_demand(&mut app)?;",
            "let result = event_loop.run_app_on_demand(&mut app);\n" + mark("event_loop_return", "u64::from(result.is_ok())") + "\nresult?;")
    winit = root / "egui-winit-0.33.3/src/lib.rs"
    replace(winit, "info.events.push(egui::ViewportEvent::Close);", "info.events.push(egui::ViewportEvent::Close);\n" + mark("close_processed"))

    replace(glow, "fn save_and_destroy(&mut self) {", "fn save_and_destroy(&mut self) {\n" + mark("destroy_enter"))
    replace(glow, "            running.integration.save(", mark("save_enter") + "\n            running.integration.save(")
    original = "            running.app.on_exit(Some(running.painter.borrow().gl()));"
    replace(glow, original, mark("save_return") + "\n" + mark("on_exit_enter") + "\n" + original + "\n" + mark("on_exit_return"))
    original = "            running.painter.borrow_mut().destroy();\n        }\n    }"
    replace(glow, original, mark("painter_destroy_enter") + "\n            running.painter.borrow_mut().destroy();\n" + mark("painter_destroy_return") + "\n" + mark("running_drop_enter") + "\ndrop(running);\n" + mark("running_drop_return") + "\n        }\n" + mark("destroy_return") + "\n    }")
    original = "        if let Some(running) = &mut self.running {\n            Ok(running.on_window_event(window_id, &event))"
    replace(glow, original, 't082_trace::mark("window_event_kind", match &event { winit::event::WindowEvent::Destroyed => 1, winit::event::WindowEvent::CloseRequested => 2, winit::event::WindowEvent::RedrawRequested => 3, _ => 0 });\n' + mark("window_event_running", "u64::from(self.running.is_some())") + "\n" + original)
    original = "Ok(running.on_window_event(window_id, &event))\n        } else {\n            Ok(EventResult::Exit)"
    replace(glow, original, "Ok(running.on_window_event(window_id, &event))\n        } else {\n" + mark("window_event_exit") + "\n            Ok(EventResult::Exit)")
    native = root / "winit-0.30.12/src/platform_impl/linux/x11/mod.rs"
    original = "        if let Ok(c) = xconn.xcb_connection().destroy_window(window.id().0 as xproto::Window) {\n            c.ignore_error();\n        }"
    replace(native, original, mark("window_drop_enter", "window.id().0 as u64") + "\n" + mark("destroy_request_enter", "window.id().0 as u64") + "\nlet request = xconn.xcb_connection().destroy_window(window.id().0 as xproto::Window);\n" + mark("destroy_request_return", "u64::from(request.is_ok())") + "\nif let Ok(c) = request { c.ignore_error(); }\n" + mark("window_drop_return", "window.id().0 as u64"))
    poll = root / "winit-0.30.12/src/platform_impl/linux/x11/event_processor.rs"
    original = "        let result = unsafe { (window_target.xconn.xlib.XPending)(window_target.xconn.display) };"
    replace(poll, original, mark("xpending_enter") + "\n" + original + "\n" + mark("xpending_return", "u64::from(result != 0)"))
    xcb = root / "x11rb-0.13.2/src/xcb_ffi/mod.rs"
    original = "            let reply = raw_ffi::xcb_wait_for_reply64(self.as_ptr(), sequence, &mut error);"
    replace(xcb, original, 't082_trace::connection(self.as_ptr() as usize);\n' + mark("reply_wait_enter", "sequence") + "\n" + original + "\n" + mark("reply_wait_return", "sequence"))
    original = "        let res = unsafe { raw_ffi::xcb_flush(self.conn.as_ptr()) };"
    replace(xcb, original, 't082_trace::connection(self.conn.as_ptr() as usize);\n' + mark("xcb_flush_enter") + "\n" + original + "\n" + mark("xcb_flush_return", "u64::from(res != 0)"))


    original = "        integration.maybe_autosave(app.as_mut(), Some(&window));"
    replace(glow, original, mark("autosave_enter") + "\n" + original + "\n" + mark("autosave_return"))
    replace(glow, "        if window.is_minimized() == Some(true) {",
            mark("minimized_enter") + "\nlet minimized = window.is_minimized();\n" + mark("minimized_return") + "\n        if minimized == Some(true) {")
    original = "        let error = unsafe { raw_ffi::xcb_request_check(self.as_ptr(), cookie) };"
    replace(xcb, original, 't082_trace::connection(self.as_ptr() as usize);\n' + mark("request_check_enter", "sequence") + "\n" + original + "\n" + mark("request_check_return", "sequence"))
    original = "            let event = raw_ffi::xcb_wait_for_event(self.conn.as_ptr());"
    replace(xcb, original, 't082_trace::connection(self.conn.as_ptr() as usize);\n' + mark("xcb_event_wait_enter") + "\n" + original + "\n" + mark("xcb_event_wait_return"))


def build_env(environ):
    """Cargo's environment: no Sophia or Hagia state, and the caller's
    CARGO_BUILD_JOBS or every CPU, at the caller's priority."""
    env = {k: v for k, v in environ.items() if not k.startswith(("SOPHIA_", "HAGIA_"))}
    env.setdefault("CARGO_BUILD_JOBS", str(os.cpu_count() or 1))
    return env

def build(args):
    dest = args.output.resolve()
    dest.mkdir(parents=True, exist_ok=False)
    for name, digest in ARCHIVES.items():
        archive = args.sources / (name + "-review.crate")
        if hashlib.sha256(archive.read_bytes()).hexdigest() != digest:
            raise RuntimeError("Archive identity mismatch: " + name)
    source = dest / "source"
    source.mkdir()
    for name in ARCHIVES:
        with tarfile.open(args.sources / (name + "-review.crate")) as archive:
            archive.extractall(source, filter="data")
    shutil.copytree(source / "pinentry-egui-0.1.1", dest / "baseline")
    def versions(path):
        return {(p["name"], p["version"]) for p in tomllib.loads(path.read_text())["package"] if p["name"] != "t082-trace"}
    locked_versions = versions(dest / "baseline/Cargo.lock")
    tool_hashes = {name: hashlib.sha256((HERE / name).read_bytes()).hexdigest() for name in ("build.py", "trace.rs", "clipboard_trace.py")}
    helper = source / "t082-trace"
    (helper / "src").mkdir(parents=True)
    (helper / "Cargo.toml").write_text('[package]\nname="t082-trace"\nversion="0.0.0"\nedition="2021"\n')
    shutil.copy2(HERE / "trace.rs", helper / "src/lib.rs")
    for name in ARCHIVES:
        path = source / name / "Cargo.toml"
        with path.open("a") as out:
            out.write('\n[dependencies.t082-trace]\npath="../t082-trace"\n')
    pin = source / "pinentry-egui-0.1.1"
    for project in (dest / "baseline", pin):
        with (project / "Cargo.toml").open("a") as out:
            out.write("\n[workspace]\n")
    with (pin / "Cargo.toml").open("a") as out:
        out.write('\n[dependencies.raw-window-handle]\nversion="0.6"\n'
                  '\n[patch.crates-io]\neframe={path="../eframe-0.33.3"}\n'
                  'egui-winit={path="../egui-winit-0.33.3"}\n'
                  'winit={path="../winit-0.30.12"}\nx11rb={path="../x11rb-0.13.2"}\n'
                  'arboard={path="../arboard-3.6.1"}\n')
    instrument(source)
    clipboard_trace.instrument(source, replace, mark)
    (pin / "src/clipboard_probe.rs").write_text(clipboard_trace.CLIENT)
    with (pin / "Cargo.toml").open("a") as out:
        out.write('\n[dependencies.arboard]\nversion="=3.6.1"\ndefault-features=false\nfeatures=["image-data"]\n[[bin]]\nname="clipboard-probe"\npath="src/clipboard_probe.rs"\n')
    env = build_env(os.environ)
    target = args.target.resolve() if args.target else dest / "target"
    env["CARGO_TARGET_DIR"] = str(target)
    command = ["cargo", "build", "--offline", "--release"]
    for name, project in (("baseline", dest / "baseline"), ("instrumented", pin)):
        # Path patches change lockfile source identity, never registry versions.
        with (dest / (name + "-build.log")).open("w") as log:
            subprocess.run(command, cwd=project, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
        shutil.copy2(target / "release/pinentry-egui", dest / ("pinentry-" + name))
    shutil.copy2(target / "release/clipboard-probe", dest / "clipboard-probe")
    if versions(dest / "baseline/Cargo.lock") != locked_versions or versions(pin / "Cargo.lock") != locked_versions:
        raise RuntimeError("Registry dependency versions drifted")
    identity = {"archives": ARCHIVES, "generator_sha256": tool_hashes,
                "cargo_jobs": env["CARGO_BUILD_JOBS"], "nice": os.nice(0),
                "generated_source_sha256": {str(p.relative_to(dest)): hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(source.rglob("*")) if p.is_file() and (p.suffix == ".rs" or p.name in ("Cargo.toml", "Cargo.lock"))},
                "tools_base_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=HERE, text=True).strip(),
                "binaries": {name: hashlib.sha256((dest / name).read_bytes()).hexdigest() for name in ("pinentry-baseline", "pinentry-instrumented", "clipboard-probe")}}
    (dest / "identity.json").write_text(json.dumps(identity, indent=2) + "\n")
    print(dest)

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sources", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--target", type=Path, help="Optional shared Cargo build cache")
    build(parser.parse_args())
