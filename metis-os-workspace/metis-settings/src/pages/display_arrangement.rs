//! macOS-style draggable monitor arrangement preview for Settings → Display.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use metis_config::{load_outputs_config, output_prefs, save_outputs_config, OutputsConfig};
use metis_protocol::OutputInfo;

const CANVAS_MIN_H: i32 = 220;
const PAD: f64 = 20.0;
const SNAP_PX: f64 = 18.0;
const TAP_THRESHOLD_PX: f64 = 6.0;

const BLOCK_COLORS: &[&str] = &[
    "metis-display-block-0",
    "metis-display-block-1",
    "metis-display-block-2",
    "metis-display-block-3",
];

#[derive(Clone)]
struct BlockState {
    name: String,
    logical_x: i32,
    logical_y: i32,
    width: i32,
    height: i32,
    label: String,
    primary: bool,
    color_idx: usize,
}

pub struct ArrangementCanvas {
    root: gtk::Box,
    canvas: gtk::Fixed,
    hint: gtk::Label,
    cfg: Rc<RefCell<OutputsConfig>>,
    outputs: Rc<RefCell<Vec<OutputInfo>>>,
    selected: Rc<RefCell<usize>>,
    on_select: Rc<dyn Fn(usize)>,
    on_pending_changed: Rc<dyn Fn(bool)>,
    blocks: Rc<RefCell<Vec<BlockState>>>,
    committed_blocks: Rc<RefCell<Vec<BlockState>>>,
    block_widgets: Rc<RefCell<Vec<gtk::Frame>>>,
    scale: Rc<RefCell<f64>>,
    origin: Rc<RefCell<(i32, i32)>>,
    pending: Rc<RefCell<bool>>,
    draggable: Rc<RefCell<bool>>,
    trial_backup: Rc<RefCell<Option<OutputsConfig>>>,
    canvas_size: Rc<RefCell<(f64, f64)>>,
    resize_debounce: Rc<RefCell<Option<glib::SourceId>>>,
}

impl ArrangementCanvas {
    pub fn new(
        cfg: Rc<RefCell<OutputsConfig>>,
        outputs: Rc<RefCell<Vec<OutputInfo>>>,
        selected: Rc<RefCell<usize>>,
        on_select: Rc<dyn Fn(usize)>,
        on_pending_changed: Rc<dyn Fn(bool)>,
    ) -> Rc<Self> {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        root.add_css_class("metis-display-arrangement");

        let hint = gtk::Label::new(None);
        hint.set_xalign(0.0);
        hint.add_css_class("metis-settings-hint");
        root.append(&hint);

        let canvas = gtk::Fixed::builder()
            .height_request(CANVAS_MIN_H)
            .hexpand(true)
            .build();
        canvas.add_css_class("metis-display-arrangement-canvas");
        canvas.set_hexpand(true);
        root.append(&canvas);

        let this = Rc::new(Self {
            root,
            canvas,
            hint,
            cfg,
            outputs,
            selected,
            on_select,
            on_pending_changed,
            blocks: Rc::new(RefCell::new(Vec::new())),
            committed_blocks: Rc::new(RefCell::new(Vec::new())),
            block_widgets: Rc::new(RefCell::new(Vec::new())),
            scale: Rc::new(RefCell::new(1.0)),
            origin: Rc::new(RefCell::new((0, 0))),
            pending: Rc::new(RefCell::new(false)),
            draggable: Rc::new(RefCell::new(false)),
            trial_backup: Rc::new(RefCell::new(None)),
            canvas_size: Rc::new(RefCell::new((480.0, CANVAS_MIN_H as f64))),
            resize_debounce: Rc::new(RefCell::new(None)),
        });
        {
            let canvas = this.canvas.clone();
            let this_w = this.clone();
            canvas.connect_notify_local(Some("width"), move |widget, _| {
                let alloc = widget.allocation();
                if alloc.width() > 0 && alloc.height() > 0 {
                    this_w.schedule_layout_for_size(alloc.width() as f64, alloc.height() as f64);
                }
            });
        }
        this.rebuild_blocks();
        this
    }

    fn schedule_layout_for_size(self: &Rc<Self>, width: f64, height: f64) {
        let prev = *self.canvas_size.borrow();
        if (prev.0 - width).abs() < 1.0 && (prev.1 - height).abs() < 1.0 {
            return;
        }
        *self.canvas_size.borrow_mut() = (width, height);

        let mut debounce = self.resize_debounce.borrow_mut();
        if let Some(id) = debounce.take() {
            id.remove();
        }
        let this = self.clone();
        let id = glib::timeout_add_local(std::time::Duration::from_millis(32), move || {
            *this.resize_debounce.borrow_mut() = None;
            if this.block_widgets.borrow().is_empty() {
                return glib::ControlFlow::Break;
            }
            this.recompute_layout();
            glib::ControlFlow::Break
        });
        *debounce = Some(id);
    }

    fn canvas_dims(self: &Rc<Self>) -> (f64, f64) {
        let alloc = self.canvas.allocation();
        if alloc.width() > 0 && alloc.height() > 0 {
            (alloc.width() as f64, alloc.height() as f64)
        } else {
            *self.canvas_size.borrow()
        }
    }

    fn recompute_layout(self: &Rc<Self>) {
        let blocks = self.blocks.borrow().clone();
        if blocks.is_empty() {
            return;
        }
        let (canvas_w, canvas_h) = self.canvas_dims();
        let (scale, min_x, min_y) = fit_scale(&blocks, canvas_w, canvas_h);
        *self.scale.borrow_mut() = scale;
        *self.origin.borrow_mut() = (min_x, min_y);

        for (block, widget) in blocks.iter().zip(self.block_widgets.borrow().iter()) {
            let (cw, ch) = block_canvas_size(block, scale);
            widget.set_size_request(cw.round().max(1.0) as i32, ch.round().max(1.0) as i32);
            let (cx, cy) = logical_to_canvas(block.logical_x, block.logical_y, min_x, min_y, scale);
            let (cx, cy) = clamp_canvas_point(cx, cy, cw, ch, canvas_w, canvas_h);
            self.canvas.move_(widget, cx, cy);
        }
    }

    pub fn widget(self: &Rc<Self>) -> &gtk::Box {
        &self.root
    }

    pub fn output_count(&self) -> usize {
        self.outputs.borrow().len()
    }

    pub fn has_pending(&self) -> bool {
        *self.pending.borrow()
    }

    pub fn in_trial(&self) -> bool {
        self.trial_backup.borrow().is_some()
    }

    /// Apply pending layout to disk (not yet confirmed). `force` allows saving
    /// when only other display fields (resolution, etc.) changed.
    pub fn begin_trial(self: &Rc<Self>, force: bool) -> bool {
        if self.in_trial() {
            return false;
        }
        if !*self.pending.borrow() && !force {
            return false;
        }
        let backup = load_outputs_config();
        let cfg = self.cfg.borrow().clone();
        if let Err(err) = save_outputs_config(&cfg) {
            tracing::warn!(%err, "failed to save output layout");
            return false;
        }
        *self.trial_backup.borrow_mut() = Some(backup);
        self.set_pending(false);
        true
    }

    /// User accepted the trial arrangement in the confirmation dialog.
    pub fn confirm_trial(self: &Rc<Self>) {
        if !self.in_trial() {
            return;
        }
        *self.committed_blocks.borrow_mut() = self.blocks.borrow().clone();
        *self.trial_backup.borrow_mut() = None;
    }

    /// User rejected the trial arrangement or the confirmation timer expired.
    pub fn cancel_trial(self: &Rc<Self>) {
        let Some(backup) = self.trial_backup.borrow_mut().take() else {
            return;
        };
        *self.cfg.borrow_mut() = backup.clone();
        if let Err(err) = save_outputs_config(&backup) {
            tracing::warn!(%err, "failed to restore output layout");
        }
        *self.blocks.borrow_mut() = self.committed_blocks.borrow().clone();
        self.reposition_widgets();
    }

    /// Discard in-memory edits and restore the last committed preview.
    pub fn revert_layout(self: &Rc<Self>) {
        if self.in_trial() {
            self.cancel_trial();
            return;
        }
        *self.cfg.borrow_mut() = load_outputs_config();
        *self.blocks.borrow_mut() = self.committed_blocks.borrow().clone();
        self.set_pending(false);
        self.reposition_widgets();
    }

    fn set_pending(self: &Rc<Self>, dirty: bool) {
        if *self.pending.borrow() == dirty {
            return;
        }
        *self.pending.borrow_mut() = dirty;
        (self.on_pending_changed)(dirty);
    }

    /// Full rebuild when the output list changes.
    pub fn rebuild_blocks(self: &Rc<Self>) {
        while let Some(child) = self.canvas.first_child() {
            self.canvas.remove(&child);
        }
        self.block_widgets.borrow_mut().clear();
        if !self.in_trial() {
            self.set_pending(false);
        }

        let list = self.outputs.borrow();
        if list.is_empty() {
            self.hint.set_label(
                "No displays detected — start a Metis session or click Detect displays.",
            );
            *self.draggable.borrow_mut() = false;
            return;
        }

        let can_arrange = list.len() >= 2 && !self.in_trial();
        *self.draggable.borrow_mut() = can_arrange;
        if self.in_trial() {
            self.hint.set_label(
                "Confirm the new arrangement in the dialog. Changes revert automatically if you do not accept.",
            );
        } else if can_arrange {
            self.hint.set_label(
                "Drag displays to match their physical positions, then click Save display settings \
         at the bottom of the page. This controls how the pointer moves between screens.",
            );
        } else {
            self.hint.set_label(
                "Single display preview. Connect another monitor to arrange relative positions.",
            );
        }

        let blocks = build_blocks(&list, &self.cfg.borrow());
        if blocks.is_empty() {
            return;
        }

        *self.blocks.borrow_mut() = blocks.clone();
        if !self.in_trial() {
            *self.committed_blocks.borrow_mut() = blocks.clone();
        }

        let sel = *self.selected.borrow();
        let draggable = *self.draggable.borrow();
        let mut widgets = Vec::with_capacity(blocks.len());
        for (idx, block) in blocks.iter().enumerate() {
            let widget = build_block_widget(block, idx == sel);
            if draggable {
                wire_drag(self, &widget, idx);
            } else {
                wire_select(self, &widget, idx);
            }
            widgets.push(widget.clone());
            self.canvas.put(&widget, PAD, PAD);
        }
        *self.block_widgets.borrow_mut() = widgets;
        self.recompute_layout();
    }

    pub fn set_selected(self: &Rc<Self>, index: usize) {
        for (idx, widget) in self.block_widgets.borrow().iter().enumerate() {
            if idx == index {
                widget.add_css_class("metis-display-block-selected");
            } else {
                widget.remove_css_class("metis-display-block-selected");
            }
        }
    }

    fn reposition_widgets(self: &Rc<Self>) {
        self.recompute_layout();
    }

    /// Refresh block positions from the latest compositor output list (after apply).
    pub fn sync_positions(self: &Rc<Self>) {
        let list = self.outputs.borrow();
        let blocks = build_blocks(&list, &self.cfg.borrow());
        if blocks.len() != self.block_widgets.borrow().len() {
            self.rebuild_blocks();
            return;
        }
        *self.blocks.borrow_mut() = blocks.clone();
        if !self.in_trial() {
            *self.committed_blocks.borrow_mut() = blocks.clone();
        }
        self.reposition_widgets();
    }
}

fn build_blocks(list: &[OutputInfo], cfg: &OutputsConfig) -> Vec<BlockState> {
    list.iter()
        .enumerate()
        .map(|(i, out)| {
            let prefs = output_prefs(cfg, &out.name);
            let (logical_x, logical_y) = if list.len() >= 2 {
                match (prefs.layout_x, prefs.layout_y) {
                    (Some(x), Some(y)) => (x, y),
                    _ => (out.rect.x, out.rect.y),
                }
            } else {
                (0, 0)
            };
            BlockState {
                name: out.name.clone(),
                logical_x,
                logical_y,
                width: out.rect.width.max(1),
                height: out.rect.height.max(1),
                label: short_label(out, i),
                primary: out.primary,
                color_idx: i % BLOCK_COLORS.len(),
            }
        })
        .collect()
}

fn short_label(out: &OutputInfo, index: usize) -> String {
    let name = if !out.make.is_empty() || !out.model.is_empty() {
        format!("{} {}", out.make.trim(), out.model.trim())
            .trim()
            .to_string()
    } else {
        out.name.clone()
    };
    if name.is_empty() {
        format!("Display {}", index + 1)
    } else {
        name
    }
}

fn fit_scale(blocks: &[BlockState], canvas_w: f64, canvas_h: f64) -> (f64, i32, i32) {
    let (min_x, min_y, max_x, max_y) = bounds(blocks);
    let bw = (max_x - min_x).max(1) as f64;
    let bh = (max_y - min_y).max(1) as f64;
    let inner_w = (canvas_w - PAD * 2.0).max(1.0);
    let inner_h = (canvas_h - PAD * 2.0).max(1.0);
    let scale = (inner_w / bw).min(inner_h / bh).max(0.01);
    (scale, min_x, min_y)
}

fn block_canvas_size(block: &BlockState, scale: f64) -> (f64, f64) {
    (
        (block.width as f64 * scale).max(1.0),
        (block.height as f64 * scale).max(1.0),
    )
}

fn clamp_canvas_point(
    cx: f64,
    cy: f64,
    cw: f64,
    ch: f64,
    canvas_w: f64,
    canvas_h: f64,
) -> (f64, f64) {
    let max_x = (canvas_w - cw - PAD).max(PAD);
    let max_y = (canvas_h - ch - PAD).max(PAD);
    (cx.clamp(PAD, max_x), cy.clamp(PAD, max_y))
}

fn bounds(blocks: &[BlockState]) -> (i32, i32, i32, i32) {
    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;
    for b in blocks {
        min_x = min_x.min(b.logical_x);
        min_y = min_y.min(b.logical_y);
        max_x = max_x.max(b.logical_x + b.width);
        max_y = max_y.max(b.logical_y + b.height);
    }
    (min_x, min_y, max_x, max_y)
}

fn logical_to_canvas(x: i32, y: i32, min_x: i32, min_y: i32, scale: f64) -> (f64, f64) {
    let cx = PAD + (x - min_x) as f64 * scale;
    let cy = PAD + (y - min_y) as f64 * scale;
    (cx, cy)
}

fn canvas_to_logical(cx: f64, cy: f64, min_x: i32, min_y: i32, scale: f64) -> (i32, i32) {
    let x = min_x + ((cx - PAD) / scale).round() as i32;
    let y = min_y + ((cy - PAD) / scale).round() as i32;
    (x, y)
}

fn build_block_widget(block: &BlockState, selected: bool) -> gtk::Frame {
    let frame = gtk::Frame::builder().build();
    frame.add_css_class("metis-display-block");
    frame.add_css_class(BLOCK_COLORS[block.color_idx]);
    if selected {
        frame.add_css_class("metis-display-block-selected");
    }
    if block.primary {
        frame.add_css_class("metis-display-block-primary");
    }

    let col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .build();

    if block.primary {
        let bar = gtk::Box::builder().height_request(6).build();
        bar.add_css_class("metis-display-block-menubar");
        col.append(&bar);
    }

    let label = gtk::Label::new(Some(&block.label));
    label.set_wrap(true);
    label.set_justify(gtk::Justification::Center);
    label.set_max_width_chars(14);
    label.add_css_class("metis-display-block-label");
    col.append(&label);

    frame.set_child(Some(&col));
    frame
}

fn wire_select(canvas: &Rc<ArrangementCanvas>, widget: &gtk::Frame, index: usize) {
    let gesture = gtk::GestureClick::new();
    gesture.connect_pressed({
        let canvas = canvas.clone();
        move |_, _, _, _| {
            *canvas.selected.borrow_mut() = index;
            canvas.set_selected(index);
            (canvas.on_select)(index);
        }
    });
    widget.add_controller(gesture);
}

fn wire_drag(canvas: &Rc<ArrangementCanvas>, widget: &gtk::Frame, index: usize) {
    let start_canvas = Rc::new(RefCell::new((0.0_f64, 0.0_f64)));
    let drag_total = Rc::new(RefCell::new((0.0_f64, 0.0_f64)));
    let drag = gtk::GestureDrag::new();
    drag.set_button(1);
    drag.connect_drag_begin({
        let widget = widget.clone();
        let start_canvas = start_canvas.clone();
        let drag_total = drag_total.clone();
        move |_, x, y| {
            let alloc = widget.allocation();
            start_canvas.replace((alloc.x() as f64 + x, alloc.y() as f64 + y));
            drag_total.replace((0.0, 0.0));
        }
    });
    drag.connect_drag_update({
        let widget = widget.clone();
        let start_canvas = start_canvas.clone();
        let drag_total = drag_total.clone();
        let fixed = canvas.canvas.clone();
        let canvas = canvas.clone();
        move |_, offset_x, offset_y| {
            drag_total.replace((offset_x, offset_y));
            let (sx, sy) = *start_canvas.borrow();
            let nx = (sx + offset_x).round();
            let ny = (sy + offset_y).round();
            let alloc = widget.allocation();
            let (canvas_w, canvas_h) = canvas.canvas_dims();
            let (nx, ny) = clamp_canvas_point(
                nx,
                ny,
                alloc.width() as f64,
                alloc.height() as f64,
                canvas_w,
                canvas_h,
            );
            fixed.move_(&widget, nx, ny);
        }
    });
    drag.connect_drag_end({
        let widget = widget.clone();
        let canvas = canvas.clone();
        let drag_total = drag_total.clone();
        move |_, _, _| {
            if canvas.in_trial() {
                return;
            }
            let (ox, oy) = *drag_total.borrow();
            let moved = ox.hypot(oy) >= TAP_THRESHOLD_PX;

            if !moved {
                *canvas.selected.borrow_mut() = index;
                canvas.set_selected(index);
                (canvas.on_select)(index);
                return;
            }

            let mut cx = widget.allocation().x() as f64;
            let mut cy = widget.allocation().y() as f64;
            snap_canvas_position(
                index,
                &mut cx,
                &mut cy,
                &canvas.blocks.borrow(),
                *canvas.scale.borrow(),
                *canvas.origin.borrow(),
            );

            let alloc = widget.allocation();
            let (canvas_w, canvas_h) = canvas.canvas_dims();
            let (cx, cy) = clamp_canvas_point(
                cx,
                cy,
                alloc.width() as f64,
                alloc.height() as f64,
                canvas_w,
                canvas_h,
            );

            let (min_x, min_y) = *canvas.origin.borrow();
            let (logical_x, logical_y) =
                canvas_to_logical(cx, cy, min_x, min_y, *canvas.scale.borrow());

            {
                let mut blocks = canvas.blocks.borrow_mut();
                if let Some(b) = blocks.get_mut(index) {
                    b.logical_x = logical_x;
                    b.logical_y = logical_y;
                }
            }

            {
                let blocks = canvas.blocks.borrow();
                let Some(block) = blocks.get(index) else {
                    return;
                };
                let mut c = canvas.cfg.borrow_mut();
                let entry = c.outputs.entry(block.name.clone()).or_default();
                entry.layout_x = Some(logical_x);
                entry.layout_y = Some(logical_y);
            }

            canvas.canvas.move_(&widget, cx.round(), cy.round());
            canvas.set_pending(true);
        }
    });
    widget.add_controller(drag);
}

fn snap_canvas_position(
    moved_idx: usize,
    cx: &mut f64,
    cy: &mut f64,
    blocks: &[BlockState],
    scale: f64,
    origin: (i32, i32),
) {
    let (min_x, min_y) = origin;
    let moved = &blocks[moved_idx];
    let mw = moved.width as f64 * scale;
    let mh = moved.height as f64 * scale;

    for (i, other) in blocks.iter().enumerate() {
        if i == moved_idx {
            continue;
        }
        let (ox, oy) = logical_to_canvas(other.logical_x, other.logical_y, min_x, min_y, scale);
        let ow = other.width as f64 * scale;
        let oh = other.height as f64 * scale;

        if (*cx - (ox + ow)).abs() < SNAP_PX {
            *cx = ox + ow;
        }
        if ((*cx + mw) - ox).abs() < SNAP_PX {
            *cx = ox - mw;
        }
        if (*cy - (oy + oh)).abs() < SNAP_PX {
            *cy = oy + oh;
        }
        if ((*cy + mh) - oy).abs() < SNAP_PX {
            *cy = oy - mh;
        }
    }
}
