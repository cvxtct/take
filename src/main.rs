mod capture;

use capture::{AppInfo, Event};
use objc2::{
    DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject, Sel},
    sel,
};
use objc2_app_kit::*;
use objc2_foundation::{
    MainThreadMarker, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize,
    NSString, NSTimer,
};
use std::{
    cell::{OnceCell, RefCell},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

fn text(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}
fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}
fn default_folder() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| {
            p.ancestors()
                .find(|p| p.extension().is_some_and(|x| x == "app"))
                .and_then(|p| p.parent())
                .map(|p| p.join("recordings"))
        })
        .unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_default()
                .join("recordings")
        })
}
fn ink(r: f64, g: f64, b: f64) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.)
}
#[derive(Default)]
struct CanvasState {
    levels: RefCell<Vec<f64>>,
}
define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = CanvasState]
    struct Canvas;
    unsafe impl NSObjectProtocol for Canvas {}
    impl Canvas {
        #[unsafe(method(drawRect:))]
        fn draw(&self, _dirty: NSRect) {
            ink(0.035, 0.043, 0.046).setFill();
            NSBezierPath::fillRect(self.bounds());
            ink(0.065, 0.078, 0.080).setFill();
            for frame in [rect(20., 86., 480., 500.), rect(516., 86., 484., 500.)] {
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(frame, 22., 22.).fill();
            }
            let levels = self.ivars().levels.borrow();
            for i in 0..100 {
                let peak = levels.get(i).copied().unwrap_or(0.);
                let height = (peak.sqrt() * 150.).max(2.);
                if i == 99 { ink(1., 0.27, 0.16).setFill(); }
                else { ink(0.66, 0.77, 0.74).setFill(); }
                NSBezierPath::fillRect(rect(44. + i as f64 * 4.3, 240. - height / 2., 1.5, height));
            }
        }
    }
);
struct Clip {
    path: PathBuf,
    duration: String,
}
struct State {
    apps: Vec<AppInfo>,
    clips: Vec<Clip>,
    busy: bool,
    recording: bool,
    closing: bool,
    stop: Arc<AtomicBool>,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
    deadline: Option<Instant>,
    started: Option<Instant>,
    saved: Option<PathBuf>,
}
impl Default for State {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            apps: vec![],
            clips: vec![],
            busy: false,
            recording: false,
            closing: false,
            stop: Arc::new(AtomicBool::new(false)),
            tx,
            rx,
            deadline: None,
            started: None,
            saved: None,
        }
    }
}
struct Controls {
    window: Retained<NSWindow>,
    apps: Retained<NSPopUpButton>,
    refresh: Retained<NSButton>,
    unlimited: Retained<NSButton>,
    seconds: Retained<NSTextField>,
    presets: Vec<Retained<NSButton>>,
    folder: Retained<NSTextField>,
    record: Retained<NSButton>,
    play: Retained<NSButton>,
    reveal: Retained<NSButton>,
    status: Retained<NSTextField>,
    clock: Retained<NSTextField>,
    canvas: Retained<Canvas>,
    clip_document: Retained<NSView>,
    clip_count: Retained<NSTextField>,
}
#[derive(Default)]
struct Ivars {
    state: RefCell<State>,
    controls: OnceCell<Controls>,
    timer: OnceCell<Retained<NSTimer>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct Delegate;
    unsafe impl NSObjectProtocol for Delegate {}
    unsafe impl NSApplicationDelegate for Delegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn launched(&self, _notification: &NSNotification) { self.build_window(); }
        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn terminate_after_close(&self, _app: &NSApplication) -> bool { true }
        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self, _app: &NSApplication) -> NSApplicationTerminateReply {
            if self.request_stop_for_close() { NSApplicationTerminateReply::TerminateCancel }
            else { NSApplicationTerminateReply::TerminateNow }
        }
    }
    unsafe impl NSWindowDelegate for Delegate {
        #[unsafe(method(windowShouldClose:))]
        fn should_close(&self, _window: &NSWindow) -> bool { !self.request_stop_for_close() }
    }
    impl Delegate {
        #[unsafe(method(refresh:))]
        fn refresh(&self, _sender: &AnyObject) {
            let mut state = self.ivars().state.borrow_mut();
            if state.busy { return; }
            state.busy = true;
            let tx = state.tx.clone();
            drop(state);
            self.status("Loading apps… Check for a macOS capture permission prompt.");
            self.sync_controls();
            std::thread::spawn(move || capture::list_apps(tx));
        }
        #[unsafe(method(toggleDuration:))]
        fn toggle_duration(&self, _sender: &AnyObject) { self.sync_controls(); }
        #[unsafe(method(preset:))]
        fn preset(&self, sender: &NSButton) {
            if let Some(c) = self.ivars().controls.get() { c.seconds.setStringValue(&text(&sender.tag().to_string())); }
        }
        #[unsafe(method(record:))]
        fn record(&self, _sender: &AnyObject) {
            let c = self.ivars().controls.get().unwrap();
            let mut state = self.ivars().state.borrow_mut();
            if state.busy {
                state.stop.store(true, Ordering::Relaxed);
                drop(state);
                self.status("Stopping and saving…");
                c.record.setEnabled(false);
                return;
            }
            let duration = match capture::recording_duration(&c.seconds.stringValue().to_string(), c.unlimited.state() == NSControlStateValueOn) {
                Ok(duration) => duration,
                Err(error) => { drop(state); self.status(&error); return; }
            };
            let Some(app) = state.apps.get(c.apps.indexOfSelectedItem() as usize).cloned() else { return; };
            let folder = c.folder.stringValue().to_string();
            if folder.trim().is_empty() { drop(state); self.status("Enter a save folder first."); return; }
            state.busy = true; state.saved = None;
            state.stop = Arc::new(AtomicBool::new(false));
            let stop = state.stop.clone(); let tx = state.tx.clone();
            drop(state);
            c.canvas.ivars().levels.borrow_mut().clear();
            self.status("Starting capture…"); self.sync_controls();
            std::thread::spawn(move || capture::record(app.pid, duration, PathBuf::from(folder.trim()), stop, tx));
        }
        #[unsafe(method(play:))]
        fn play(&self, _sender: &AnyObject) { self.open_saved(false); }
        #[unsafe(method(reveal:))]
        fn reveal(&self, _sender: &AnyObject) { self.open_saved(true); }
        #[unsafe(method(selectClip:))]
        fn select_clip(&self, sender: &NSButton) {
            let mut state = self.ivars().state.borrow_mut();
            state.saved = state.clips.get(sender.tag() as usize).map(|c| c.path.clone());
            drop(state);
            self.render_clips(); self.sync_controls();
        }
        #[unsafe(method(reloadClips:))]
        fn reload_clips(&self, _sender: &AnyObject) { self.load_clips(); }
        #[unsafe(method(tick:))]
        fn tick(&self, _timer: &NSTimer) { self.poll(); }
    }
);
impl Delegate {
    fn status(&self, message: &str) {
        if let Some(c) = self.ivars().controls.get() {
            c.status.setStringValue(&text(message));
        }
    }
    fn request_stop_for_close(&self) -> bool {
        let mut state = self.ivars().state.borrow_mut();
        if !state.busy {
            return false;
        }
        state.closing = true;
        state.stop.store(true, Ordering::Relaxed);
        drop(state);
        self.status("Stopping and saving before closing…");
        true
    }
    fn open_saved(&self, reveal: bool) {
        if let Some(path) = &self.ivars().state.borrow().saved {
            let mut command = std::process::Command::new("open");
            if reveal {
                command.arg("-R");
            }
            if let Err(error) = command.arg(path).spawn() {
                self.status(&format!("Could not open file: {error}"));
            }
        }
    }
    fn sync_controls(&self) {
        let Some(c) = self.ivars().controls.get() else {
            return;
        };
        let state = self.ivars().state.borrow();
        c.apps.setEnabled(!state.busy);
        c.refresh.setEnabled(!state.busy);
        c.unlimited.setEnabled(!state.busy);
        c.folder.setEnabled(!state.busy);
        let timed = !state.busy && c.unlimited.state() != NSControlStateValueOn;
        c.seconds.setEnabled(timed);
        for button in &c.presets {
            button.setEnabled(timed);
        }
        c.record
            .setTitle(&text(if state.busy { "Stop & Save" } else { "Record" }));
        c.record
            .setEnabled(state.recording || (!state.busy && !state.apps.is_empty()));
        c.play.setEnabled(state.saved.is_some() && !state.busy);
        c.reveal.setEnabled(state.saved.is_some() && !state.busy);
    }
    fn poll(&self) {
        let Some(c) = self.ivars().controls.get() else {
            return;
        };
        let mut state = self.ivars().state.borrow_mut();
        let mut changed = false;
        let mut saved = false;
        while let Ok(event) = state.rx.try_recv() {
            changed = true;
            match event {
                Event::Apps(apps) => {
                    // Keep the selection on refresh when that process is still present.
                    let previous = state
                        .apps
                        .get(c.apps.indexOfSelectedItem() as usize)
                        .map(|a| a.pid);
                    state.apps = apps;
                    c.apps.removeAllItems();
                    for app in &state.apps {
                        c.apps
                            .addItemWithTitle(&text(&format!("{} ({})", app.name, app.pid)));
                    }
                    let selected = state
                        .apps
                        .iter()
                        .position(|a| Some(a.pid) == previous)
                        .or_else(|| state.apps.iter().position(|a| a.browser))
                        .unwrap_or(0);
                    if !state.apps.is_empty() {
                        c.apps.selectItemAtIndex(selected as isize);
                    }
                    state.busy = false;
                    self.status(if state.apps.is_empty() {
                        "No apps found. Open your browser, then refresh."
                    } else {
                        "Choose the browser playing your music."
                    });
                }
                Event::Started(duration) => {
                    state.recording = true;
                    state.started = Some(Instant::now());
                    state.deadline = duration.map(|s| Instant::now() + Duration::from_secs(s));
                    self.status("Recording — play the excerpt in your browser.");
                }
                Event::Level(peak) => {
                    let mut levels = c.canvas.ivars().levels.borrow_mut();
                    if levels.is_empty() {
                        levels.resize(100, 0.);
                    }
                    levels.remove(0);
                    levels.push(f64::from(peak.clamp(0., 1.)));
                    c.canvas.setNeedsDisplay(true);
                }
                Event::Saved(path, warning) => {
                    saved = true;
                    self.status(&format!(
                        "Saved {}{}",
                        path.file_name().unwrap_or_default().to_string_lossy(),
                        warning
                    ));
                    state.saved = Some(path);
                    state.busy = false;
                    state.recording = false;
                    state.started = None;
                    state.deadline = None;
                }
                Event::Error(error) => {
                    self.status(&error);
                    state.busy = false;
                    state.recording = false;
                    state.started = None;
                    state.deadline = None;
                    // Keep an error visible rather than quitting and hiding a failed save.
                    state.closing = false;
                }
            }
        }
        if let Some(started) = state.started {
            let elapsed = started.elapsed().as_secs_f64();
            c.clock.setStringValue(&text(&format!(
                "{:02}:{:04.1}",
                elapsed as u64 / 60,
                elapsed % 60.
            )));
        }
        let close = state.closing && !state.busy;
        drop(state);
        if saved {
            self.load_clips();
        }
        if changed {
            self.sync_controls();
        }
        if close {
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }
    }
    fn label(
        &self,
        parent: &NSView,
        value: &str,
        frame: NSRect,
        secondary: bool,
    ) -> Retained<NSTextField> {
        let label = NSTextField::wrappingLabelWithString(&text(value), self.mtm());
        label.setFrame(frame);
        label.setFont(Some(&NSFont::systemFontOfSize(13.)));
        if secondary {
            label.setTextColor(Some(&ink(0.63, 0.68, 0.68)));
        }
        parent.addSubview(&label);
        label
    }
    fn button(
        &self,
        parent: &NSView,
        title: &str,
        action: Sel,
        frame: NSRect,
    ) -> Retained<NSButton> {
        // Every target selector below is implemented by this main-thread delegate.
        let button = unsafe {
            NSButton::buttonWithTitle_target_action(
                &text(title),
                Some(self),
                Some(action),
                self.mtm(),
            )
        };
        button.setFrame(frame);
        parent.addSubview(&button);
        button
    }
    fn load_clips(&self) {
        let c = self.ivars().controls.get().unwrap();
        let folder = PathBuf::from(c.folder.stringValue().to_string());
        let mut paths: Vec<_> = std::fs::read_dir(folder)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav")))
            .collect();
        paths.sort_by_key(|p| std::cmp::Reverse(p.metadata().and_then(|m| m.modified()).ok()));
        let clips = paths
            .into_iter()
            .map(|path| {
                let duration = hound::WavReader::open(&path)
                    .ok()
                    .map(|r| {
                        let seconds = r.duration() as f64 / r.spec().sample_rate as f64;
                        format!("{}:{:04.1}", seconds as u64 / 60, seconds % 60.)
                    })
                    .unwrap_or_else(|| "—".into());
                Clip { path, duration }
            })
            .collect();
        self.ivars().state.borrow_mut().clips = clips;
        self.render_clips();
    }
    fn render_clips(&self) {
        let c = self.ivars().controls.get().unwrap();
        for view in c.clip_document.subviews().iter() {
            view.removeFromSuperview();
        }
        let state = self.ivars().state.borrow();
        c.clip_count
            .setStringValue(&text(&format!("{} clips", state.clips.len())));
        let height = (state.clips.len() as f64 * 46.).max(342.);
        c.clip_document.setFrame(rect(0., 0., 434., height));
        if state.clips.is_empty() {
            self.label(
                &c.clip_document,
                "Your recordings will appear here.\nRecord a clip to get started.",
                rect(10., height - 80., 410., 64.),
                true,
            );
        }
        for (i, clip) in state.clips.iter().enumerate() {
            let selected = state.saved.as_ref() == Some(&clip.path);
            let filename = clip.path.file_stem().unwrap_or_default().to_string_lossy();
            let name: String = filename.chars().take(35).collect();
            let title = format!("{}  {}", if selected { "●" } else { "○" }, name);
            let button = self.button(
                &c.clip_document,
                &title,
                sel!(selectClip:),
                rect(0., height - (i + 1) as f64 * 46., 348., 40.),
            );
            button.setTag(i as isize);
            button.setBordered(false);
            button.setContentTintColor(Some(
                (if selected {
                    ink(1., 0.36, 0.24)
                } else {
                    ink(0.84, 0.88, 0.86)
                })
                .as_ref(),
            ));
            self.label(
                &c.clip_document,
                &clip.duration,
                rect(366., height - (i + 1) as f64 * 46. + 9., 68., 22.),
                true,
            );
        }
    }
    fn build_window(&self) {
        let mtm = self.mtm();
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        self.build_menu(&app);
        let window = unsafe {
            let w = NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect(0., 0., 1020., 660.),
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable,
                NSBackingStoreType::Buffered,
                false,
            );
            w.setReleasedWhenClosed(false);
            w
        };
        window.setTitle(&text("Browser Recorder"));
        window.setAppearance(
            NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua }).as_deref(),
        );
        window.setBackgroundColor(Some(&ink(0.035, 0.043, 0.046)));
        window.setDelegate(Some(ProtocolObject::from_ref(self)));
        let canvas: Retained<Canvas> = unsafe {
            msg_send![super(Canvas::alloc(mtm).set_ivars(CanvasState::default())), initWithFrame: rect(0.,0.,1020.,660.)]
        };
        window.setContentView(Some(&canvas));
        let view: &NSView = &canvas;
        let title = self.label(view, "BROWSER RECORDER", rect(26., 614., 330., 24.), false);
        title.setFont(Some(&NSFont::boldSystemFontOfSize(14.)));
        self.label(
            view,
            "CAPTURE / LISTEN / KEEP",
            rect(365., 614., 300., 20.),
            true,
        );
        let record = self.button(view, "Record", sel!(record:), rect(816., 603., 180., 42.));
        record.setKeyEquivalent(&text("\r"));
        record.setBezelColor(Some(&ink(0.94, 0.19, 0.10)));
        record.setContentTintColor(Some(&NSColor::whiteColor()));
        let clock = self.label(view, "00:00.0", rect(42., 475., 430., 90.), false);
        clock.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
            66., -0.4,
        )));
        self.label(view, "AUDIO SOURCE", rect(44., 447., 250., 20.), true);
        let apps = NSPopUpButton::initWithFrame_pullsDown(
            NSPopUpButton::alloc(mtm),
            rect(40., 409., 320., 30.),
            false,
        );
        apps.addItemWithTitle(&text("Choose an app"));
        view.addSubview(&apps);
        let refresh = self.button(view, "Refresh", sel!(refresh:), rect(370., 407., 110., 32.));
        self.label(view, "DURATION", rect(44., 368., 120., 20.), true);
        let seconds = NSTextField::textFieldWithString(&text("30"), mtm);
        seconds.setFrame(rect(44., 335., 48., 26.));
        view.addSubview(&seconds);
        self.label(view, "sec", rect(99., 337., 30., 22.), true);
        let mut presets = vec![];
        for (i, value) in [10, 20, 30, 60].into_iter().enumerate() {
            let b = self.button(
                view,
                &format!("{value}s"),
                sel!(preset:),
                rect(139. + i as f64 * 59., 331., 55., 32.),
            );
            b.setTag(value);
            presets.push(b);
        }
        let unlimited = unsafe {
            NSButton::checkboxWithTitle_target_action(
                &text("Until I stop"),
                Some(self),
                Some(sel!(toggleDuration:)),
                mtm,
            )
        };
        unlimited.setFrame(rect(330., 365., 150., 24.));
        view.addSubview(&unlimited);
        self.label(view, "LIVE LEVEL HISTORY", rect(44., 132., 300., 20.), true);
        self.label(
            view,
            "Stereo WAV · 48 kHz · Local only",
            rect(44., 102., 410., 20.),
            true,
        );
        let heading = self.label(view, "Clips", rect(540., 538., 220., 30.), false);
        heading.setFont(Some(&NSFont::systemFontOfSize(23.)));
        let clip_count = self.label(view, "0 clips", rect(890., 542., 90., 24.), true);
        let scroll =
            NSScrollView::initWithFrame(NSScrollView::alloc(mtm), rect(538., 182., 440., 342.));
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        let clip_document = NSView::initWithFrame(NSView::alloc(mtm), rect(0., 0., 434., 342.));
        scroll.setDocumentView(Some(&clip_document));
        view.addSubview(&scroll);
        let play = self.button(view, "Play", sel!(play:), rect(536., 124., 100., 34.));
        let reveal = self.button(
            view,
            "Show in Finder",
            sel!(reveal:),
            rect(644., 124., 164., 34.),
        );
        self.button(
            view,
            "Reload",
            sel!(reloadClips:),
            rect(870., 124., 110., 34.),
        );
        self.label(
            view,
            "Select a clip to play or reveal its file.",
            rect(544., 97., 420., 20.),
            true,
        );
        self.label(view, "SAVE TO", rect(26., 50., 76., 20.), true);
        let folder =
            NSTextField::textFieldWithString(&text(&default_folder().display().to_string()), mtm);
        folder.setFrame(rect(106., 46., 888., 26.));
        view.addSubview(&folder);
        let status = self.label(
            view,
            "Refresh apps to choose your audio source. Pause other tabs in that app.",
            rect(26., 7., 966., 34.),
            true,
        );
        status.setSelectable(true);
        self.ivars()
            .controls
            .set(Controls {
                window,
                apps,
                refresh,
                unlimited,
                seconds,
                presets,
                folder,
                record,
                play,
                reveal,
                status,
                clock,
                canvas,
                clip_document,
                clip_count,
            })
            .ok();
        self.sync_controls();
        self.load_clips();
        let c = self.ivars().controls.get().unwrap();
        c.window.center();
        c.window.makeKeyAndOrderFront(None);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                0.1,
                self,
                sel!(tick:),
                None,
                true,
            )
        };
        self.ivars().timer.set(timer).ok();
    }
    fn build_menu(&self, app: &NSApplication) {
        let mtm = self.mtm();
        let menu = NSMenu::new(mtm);
        let app_item = NSMenuItem::new(mtm);
        let app_menu = NSMenu::new(mtm);
        let quit = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &text("Quit Browser Recorder"),
                Some(sel!(terminate:)),
                &text("q"),
            )
        };
        app_menu.addItem(&quit);
        app_item.setSubmenu(Some(&app_menu));
        menu.addItem(&app_item);
        let edit_item = NSMenuItem::new(mtm);
        edit_item.setTitle(&text("Edit"));
        let edit = NSMenu::initWithTitle(NSMenu::alloc(mtm), &text("Edit"));
        for (title, action, key) in [
            ("Undo", sel!(undo:), "z"),
            ("Cut", sel!(cut:), "x"),
            ("Copy", sel!(copy:), "c"),
            ("Paste", sel!(paste:), "v"),
            ("Select All", sel!(selectAll:), "a"),
        ] {
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &text(title),
                    Some(action),
                    &text(key),
                )
            };
            edit.addItem(&item);
        }
        edit_item.setSubmenu(Some(&edit));
        menu.addItem(&edit_item);
        app.setMainMenu(Some(&menu));
    }
}
fn main() {
    let mtm = MainThreadMarker::new().expect("Run the app on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    let delegate: Retained<Delegate> = unsafe {
        msg_send![
            super(Delegate::alloc(mtm).set_ivars(Ivars::default())),
            init
        ]
    };
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
}
