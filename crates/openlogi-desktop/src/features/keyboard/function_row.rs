//! The keyboard key remapper view — the Keys tab body.
//!
//! A two-pane inspector model (the "pro-tool" layout): the keyboard photo sits
//! beside a row of mouse-style callout bubbles, and clicking a key **selects**
//! it (no popover). A tall, scrollable config panel slides in on the right
//! while the keyboard physically makes room. Only one key is selected at a
//! time.
//!
//! Which keys appear is the asset's call (see [`key_points::key_surface`]): a
//! depot with control markers shows the keyboard's own HID++ controls on the
//! photo, each bound per device through [`AppState::commit_binding`] and
//! diverted by the agent while bound; a keypad whose depot marks its keys
//! puts those keycaps on the device render itself; a markerless keypad shows
//! its own keycaps drawn from the device model beside the render, bound the
//! same per-device way; any other markerless device shows the OS-hook F-row,
//! bound globally through [`AppState::commit_keyboard_binding`]. The panel
//! lists the same action catalog the mouse picker uses, plus a Power User
//! section.

#![expect(
    clippy::needless_pass_by_value,
    reason = "GPUI builders take owned Copy palette values"
)]

use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, Entity, FontWeight, Hsla,
    InteractiveElement, IntoElement, ParentElement, PathBuilder, Render, RenderOnce, Role,
    SharedString, StatefulInteractiveElement as _, Styled, Subscription, Window, canvas, div, hsla,
    point, prelude::FluentBuilder as _, px, rgb, svg,
};
use gpui_component::button::{Button, ButtonGroup, ButtonVariant, ButtonVariants as _};
use gpui_component::dialog::DialogButtonProps;
use gpui_component::tooltip::Tooltip;
use gpui_component::{
    Disableable as _, IconName, Selectable as _, Sizable as _, WindowExt as _, h_flex,
    input::InputState, v_flex,
};
use openlogi_core::binding::{Action, ButtonId, Cid, GridKey, KeypadPage, WorkflowStep};
use openlogi_core::device::DeviceKind;

use super::editors::{
    PowerUserKind, text_editor_placeholder, text_editor_seed, workflow_editor_seed,
};
use crate::app::{glow_canvas, keyboard_glow};
use crate::features::binding_editor::{
    PickFn, action_icon_path, action_rows, compact_panel, divider, editor_scroll_list,
    editor_section,
};
use crate::features::mouse::geometry::asset_dimensions_for_png;
use crate::features::profiles::profile_name;
use crate::services::assets::{GlowGeometry, ResolvedAsset};
use crate::state::{AppState, DeviceKey, DeviceRecord, StateEvent, StateEvents};
use crate::ui::action::localized_action_label;
use crate::ui::components::MenuRow;
use crate::ui::theme::{self, ACCENT_BLUE, Palette, Typography as _};
use gpui::ease_in_out;
use gpui::{Animation, AnimationExt, img};

mod key_points;

pub(crate) use key_points::KeyTarget;
#[cfg(test)]
use key_points::{EVEN_SPACING_END, EVEN_SPACING_START, key_x_fractions};
use key_points::{
    KEYCAP_H, KEYCAP_PAGING_H, KEYCAP_W, KeySurface, is_paging_cid, key_surface, keypad_canvas,
};

/// Width of the config panel (CSS px) when a key is selected.
const PANEL_W: f32 = 320.;
/// Duration of the keyboard slide + panel slide animation.
const SLIDE_MS: u64 = 180;
/// Maximum keyboard render width in the Keys inspector.
const KEYBOARD_W: f32 = 700.;
/// Render size when no asset resolved: the placeholder box.
const FALLBACK_KEYBOARD_SIZE: (f32, f32) = (KEYBOARD_W, 220.);
/// Space above the keyboard reserved for function-key callouts.
const CALLOUT_BAND_H: f32 = 118.;
/// Vertical chrome around the keyboard pane (header, tab strip, screen
/// padding, footer) — the viewport height minus this and the callout band is
/// what the render may occupy before it scales down to fit.
const KEYS_VERTICAL_RESERVE: f32 = 224.;
/// Floor on the render height so a tiny window still shows a usable model.
const KEYBOARD_MIN_IMG_H: f32 = 160.;
const KEY_CALLOUT_W: f32 = 60.;
const KEY_CALLOUT_H: f32 = 48.;
const KEY_CALLOUT_TOP_UPPER: f32 = 4.;
const KEY_CALLOUT_TOP_LOWER: f32 = 50.;
const KEY_TARGET_W: f32 = 30.;
const KEY_TARGET_H: f32 = 30.;
const KEY_HOTSPOT_DOT: f32 = 12.;

/// The function-row remapper view.
pub struct FunctionRowView {
    /// The single selected key index (0 = Esc), or `None` when nothing is
    /// selected (no panel shown).
    selected_key: Option<usize>,
    /// The hovered function-row key index, shared by callout bubbles, key hit
    /// zones, and leader lines.
    hovered_key: Option<usize>,
    /// Which power-user editor is showing in the panel, if any.
    active_editor: Option<PowerUserKind>,
    /// Lazily-created [`InputState`] for the text editors.
    text_state: Option<Entity<InputState>>,
    /// Draft copy of the Workflow steps under edit.
    workflow_draft: Vec<WorkflowStep>,
    /// The keypad binding page the grid keys are being edited on — this
    /// editor's own choice, independent of the page the keypad is showing.
    keypad_page: KeypadPage,
    /// The device [`Self::keypad_page`] was chosen on; another device starts
    /// back on page 1.
    keypad_device: Option<DeviceKey>,
    _state_obs: Subscription,
}

impl FunctionRowView {
    /// Create the view.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state_obs =
            AppState::repaint_on(cx, |event| matches!(event, StateEvent::BindingsChanged(_)));
        Self {
            selected_key: None,
            hovered_key: None,
            active_editor: None,
            text_state: None,
            workflow_draft: Vec::new(),
            keypad_page: KeypadPage::FIRST,
            keypad_device: None,
            _state_obs: state_obs,
        }
    }

    /// Select a key (or deselect with `None`), opening/closing the panel.
    pub(crate) fn select_key(&mut self, idx: Option<usize>, cx: &mut Context<Self>) {
        // Changing selection also drops any open editor + its drafts.
        if self.selected_key != idx {
            self.active_editor = None;
            self.text_state = None;
            self.workflow_draft.clear();
        }
        self.selected_key = idx;
        cx.notify();
    }

    /// Toggle a key selection from a click on either its callout or key hit
    /// target.
    pub(crate) fn click_key(&mut self, idx: usize, cx: &mut Context<Self>) {
        self.select_key(next_selection_after_click(self.selected_key, idx), cx);
    }

    pub(crate) fn set_hovered_key(&mut self, idx: Option<usize>, cx: &mut Context<Self>) {
        if self.hovered_key != idx {
            self.hovered_key = idx;
            cx.notify();
        }
    }

    /// Keep the edited page on the selected device's pages — page 1 on a new
    /// device, clamped when pages were removed — and answer the device's last
    /// page and whether removing it would drop bindings (so removal asks).
    fn sync_keypad_page(&mut self, state: Option<&AppState>) -> (KeypadPage, bool) {
        let record_key = state
            .and_then(AppState::current_record)
            .map(DeviceRecord::device_key);
        if self.keypad_device != record_key {
            self.keypad_device = record_key;
            self.keypad_page = KeypadPage::FIRST;
        }
        let last = state.map_or(KeypadPage::FIRST, AppState::current_keypad_pages);
        self.keypad_page = self.keypad_page.min(last);
        let last_bound = state.is_some_and(|state| {
            state.button_bindings().iter().any(|(button, action)| {
                button.grid_slot().is_some_and(|slot| slot.page == last) && *action != Action::None
            })
        });
        (last, last_bound)
    }

    /// Edit the grid keys on `page`.
    pub(crate) fn set_keypad_page(&mut self, page: KeypadPage, cx: &mut Context<Self>) {
        if self.keypad_page != page {
            self.keypad_page = page;
            cx.notify();
        }
    }

    pub(crate) fn open_editor(&mut self, kind: PowerUserKind, cx: &mut Context<Self>) {
        self.active_editor = Some(kind);
        self.text_state = None;
        self.workflow_draft.clear();
        cx.notify();
    }

    pub(crate) fn close_editor(&mut self, cx: &mut Context<Self>) {
        self.active_editor = None;
        self.text_state = None;
        self.workflow_draft.clear();
        cx.notify();
    }

    pub(crate) fn text_state(&self) -> Option<Entity<InputState>> {
        self.text_state.clone()
    }

    pub(crate) fn new_text_state(
        &mut self,
        seed: String,
        placeholder: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let state = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder(placeholder);
            if !seed.is_empty() {
                s.set_value(seed, window, cx);
            }
            s
        });
        self.text_state = Some(state.clone());
        state
    }

    pub(crate) fn workflow_draft(&self) -> &[WorkflowStep] {
        &self.workflow_draft
    }

    pub(crate) fn push_workflow_step(&mut self, step: WorkflowStep, cx: &mut Context<Self>) {
        self.workflow_draft.push(step);
        cx.notify();
    }

    pub(crate) fn remove_workflow_step(&mut self, idx: usize, cx: &mut Context<Self>) {
        if idx < self.workflow_draft.len() {
            self.workflow_draft.remove(idx);
            cx.notify();
        }
    }
}

impl Render for FunctionRowView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = AppState::try_read(cx);
        let asset = state.and_then(|state| state.current_record()?.asset.as_ref());
        // Without a record there is no device surface to trust, and the F-row
        // fallback is as honest as anything that shows.
        let kind = state
            .and_then(|state| state.current_record())
            .map_or(DeviceKind::Keyboard, |record| record.kind);
        let glow = state.and_then(|state| {
            state
                .current_record()
                .and_then(|record| keyboard_glow(state, record))
        });

        let viewport_h = f32::from(window.viewport_size().height);
        let image_path = asset.map(|asset| asset.image_path.clone());
        let surface = key_surface(asset, kind);
        let keypad = matches!(surface, KeySurface::Keypad(_));
        // A depot-marked keypad puts its caps on the render itself: the
        // keyboard's callout band cannot lay eleven bubbles across a
        // portrait render, and the keys sit under the user's eyes already.
        let keypad_photo =
            matches!(surface, KeySurface::Photo(_)) && matches!(kind, DeviceKind::Numpad);
        // The schematic keeps photo and keycaps on one alignment line.
        let photo = asset
            .filter(|asset| asset.png_height > 0)
            .map_or((0., 0.), |asset| {
                asset_dimensions_for_png(asset, keypad_canvas().1, 360.)
            });
        let render_size = if keypad_photo {
            keypad_photo_render_size(asset, viewport_h)
        } else {
            keyboard_render_size(asset, viewport_h)
        };
        let (last_page, last_page_bound) = self.sync_keypad_page(state);
        let pal = theme::palette(cx);
        let slots = key_slots_from_layouts(surface, state, self.keypad_page);

        // A stale selection can outlive a device switch to a shorter F-row;
        // drop it instead of indexing past the new slot list.
        if self.selected_key.is_some_and(|idx| idx >= slots.len()) {
            self.selected_key = None;
            self.active_editor = None;
            self.text_state = None;
            self.workflow_draft.clear();
        }
        let selected = self.selected_key;
        let hovered = self.hovered_key;
        let active_editor = self.active_editor;
        if let (Some(selected_idx), Some(kind)) = (selected, active_editor)
            && let Some(slot) = slots.get(selected_idx)
        {
            let current_action = state.and_then(|state| bound_action(state, &slot.target));
            match kind {
                PowerUserKind::Workflow => {
                    if self.workflow_draft.is_empty() {
                        self.workflow_draft = workflow_editor_seed(current_action.as_ref());
                    }
                }
                _ => {
                    if let Some(state) = self.text_state.clone() {
                        crate::ui::components::localize_placeholder(
                            &state,
                            text_editor_placeholder(kind),
                            window,
                            cx,
                        );
                    } else {
                        self.new_text_state(
                            text_editor_seed(current_action.as_ref(), kind),
                            text_editor_placeholder(kind),
                            window,
                            cx,
                        );
                    }
                }
            }
        }
        let view = cx.entity();
        // The same selection, panel, and editor machinery on either surface:
        // the keyboard's photo with callouts, or the keypad's photo beside
        // drawn keycaps.
        let device = if keypad {
            KeypadPane::new(slots.clone(), image_path, photo, view.clone())
                .selected(selected)
                .hovered(hovered)
                .into_any_element()
        } else if keypad_photo {
            KeypadPhotoPane::new(slots.clone(), image_path, render_size, view.clone())
                .selected(selected)
                .hovered(hovered)
                .into_any_element()
        } else {
            KeyboardPane::new(slots.clone(), image_path, glow, render_size, view.clone())
                .selected(selected)
                .hovered(hovered)
                .into_any_element()
        };
        let panel = selected.map(|selected| self.config_panel(selected, &slots, &view, cx));
        let page_bar = (keypad || keypad_photo)
            .then(|| keypad_page_bar(self.keypad_page, last_page, last_page_bound, &view, &pal));

        // The whole row animates as one: when a key is selected the right-side
        // panel grows in and the keyboard nudges left to make room.
        v_flex()
            .w_full()
            .items_center()
            .gap_3()
            .children(page_bar)
            .child(InspectorRow::new(device).panel(panel))
    }
}

/// A keypad's binding pages: pick the page the grid keys are edited on, add
/// one after the last, or remove the last. Removing a page that still binds
/// keys asks first, because its bindings go with it; an empty one just goes.
fn keypad_page_bar(
    page: KeypadPage,
    last: KeypadPage,
    last_page_bound: bool,
    view: &Entity<FunctionRowView>,
    pal: &Palette,
) -> impl IntoElement {
    let pages: Vec<KeypadPage> = last.through().collect();
    let select = {
        let view = view.clone();
        let pages = pages.clone();
        move |indices: &Vec<usize>, _: &mut Window, cx: &mut App| {
            if let Some(page) = indices.first().and_then(|index| pages.get(*index)) {
                view.update(cx, |view, cx| view.set_keypad_page(*page, cx));
            }
        }
    };
    let group = ButtonGroup::new("keypad-pages")
        .outline()
        .small()
        .children(pages.iter().map(|candidate| {
            Button::new(("keypad-page", usize::from(candidate.number())))
                .label(candidate.to_string())
                .selected(*candidate == page)
        }))
        .on_click(select);
    let added = KeypadPage::new(last.number().saturating_add(1));
    let add = {
        let view = view.clone();
        Button::new("keypad-add-page")
            .ghost()
            .small()
            .icon(IconName::Plus)
            .label(tr!("keyboard.add_page"))
            .disabled(added.is_none())
            .on_click(move |_, _, cx| {
                let Some(added) = added else { return };
                AppState::apply(cx, |state| state.commit_keypad_pages(added));
                view.update(cx, |view, cx| view.set_keypad_page(added, cx));
            })
    };
    let remaining = KeypadPage::new(last.number().saturating_sub(1));
    let remove = Button::new("keypad-remove-page")
        .ghost()
        .small()
        .label(tr!("keyboard.remove_page", page => last.number()))
        .disabled(remaining.is_none())
        .on_click(move |_, window, cx| {
            let Some(remaining) = remaining else { return };
            if !last_page_bound {
                AppState::apply(cx, |state| state.commit_keypad_pages(remaining));
                return;
            }
            window.open_alert_dialog(cx, move |alert, _, _| {
                alert
                    .title(tr!("keyboard.remove_page_question", page => last.number()))
                    .description(tr!("keyboard.remove_page_description"))
                    .button_props(
                        DialogButtonProps::default()
                            .ok_text(tr!("keyboard.remove_page", page => last.number()))
                            .ok_variant(ButtonVariant::Danger)
                            .cancel_text(tr!("common.cancel"))
                            .show_cancel(true),
                    )
                    .on_ok(move |_, _, cx| {
                        AppState::apply(cx, |state| state.commit_keypad_pages(remaining));
                        true
                    })
            });
        });
    h_flex()
        .gap_2()
        .items_center()
        .child(
            div()
                .text_caption()
                .text_color(pal.text_muted)
                .child(tr!("keyboard.pages")),
        )
        .child(group)
        .child(add)
        .child(remove)
}

fn binding_line(
    binding: SharedString,
    icon: Option<&'static str>,
    max_w: f32,
    highlighted: bool,
    pal: &Palette,
) -> gpui::Div {
    h_flex()
        .items_center()
        .justify_center()
        .gap(px(2.))
        .max_w(px(max_w))
        .when_some(icon, |row, icon| {
            row.child(
                svg()
                    .path(icon)
                    .size(px(9.))
                    .flex_none()
                    .text_color(if highlighted {
                        rgb(ACCENT_BLUE).into()
                    } else {
                        pal.text_muted
                    }),
            )
        })
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_caption()
                .text_color(if highlighted {
                    rgb(ACCENT_BLUE).into()
                } else {
                    pal.text_muted
                })
                .child(binding),
        )
}

/// The keyboard render size: the actual PNG aspect at up to [`KEYBOARD_W`]
/// wide, shrunk to fit the viewport height. Sizing off the real aspect keeps
/// `ObjectFit::Contain` from letterboxing and keeps the marker overlays
/// registered with the rendered keys — the G513 render (with wrist rest) is
/// nearly twice as tall as an MX Keys render at the same width.
fn keyboard_render_size(asset: Option<&ResolvedAsset>, viewport_h: f32) -> (f32, f32) {
    let Some(asset) = asset.filter(|a| a.png_height > 0) else {
        return FALLBACK_KEYBOARD_SIZE;
    };
    let target_h = (viewport_h - KEYS_VERTICAL_RESERVE - CALLOUT_BAND_H).max(KEYBOARD_MIN_IMG_H);
    asset_dimensions_for_png(asset, target_h, KEYBOARD_W)
}

/// One key with its resolved layout + binding.
#[derive(Clone)]
struct KeySlot {
    idx: usize,
    label: SharedString,
    target: KeyTarget,
    x_frac: f32,
    y_frac: f32,
    binding: SharedString,
    binding_icon: Option<&'static str>,
}

/// One [`KeySlot`] per displayed key, each carrying the action it is bound
/// to right now — the panes and the config panel render the same truth
/// from the same source.
/// The tab's keys with their current bindings, grid keys read on `page`.
fn key_slots_from_layouts(
    surface: KeySurface,
    state: Option<&AppState>,
    page: KeypadPage,
) -> Vec<KeySlot> {
    surface
        .slots()
        .into_iter()
        .enumerate()
        .map(|(idx, mut layout)| {
            layout.target = layout.target.on_page(page);
            let bound = state.and_then(|state| bound_action(state, &layout.target));
            KeySlot {
                idx,
                label: layout.legend.into(),
                x_frac: layout.point.x_frac,
                y_frac: layout.point.y_frac,
                binding: binding_label(bound.as_ref()),
                binding_icon: bound.as_ref().map(action_icon_path),
                target: layout.target,
            }
        })
        .collect()
}

/// The action `target` is bound to in the selected device's open profile:
/// a per-device binding (a control or keypad key) or the global F-row map
/// (a function key). `None` when the key is on its native function.
fn bound_action(state: &AppState, target: &KeyTarget) -> Option<Action> {
    match target {
        KeyTarget::Control(button) => state
            .button_bindings()
            .get(button)
            .filter(|action| **action != Action::None)
            .cloned(),
        KeyTarget::Grid(slot) => state
            .button_bindings()
            .get(&ButtonId::Grid(*slot))
            .filter(|action| **action != Action::None)
            .cloned(),
        KeyTarget::FunctionKey(trigger) => state.keyboard_bindings().get(trigger).cloned(),
    }
}

/// Persist `action` for `target`: controls and keypad keys bind per device
/// (and per open app profile) like any mouse button; a function key binds
/// globally. `None` returns a device key to its native function; for a
/// function key it clears the trigger.
pub(super) fn commit_key_action(
    state: &mut AppState,
    target: &KeyTarget,
    action: Option<Action>,
) -> StateEvents {
    match target {
        KeyTarget::Control(button) => state.commit_binding(*button, action.unwrap_or(Action::None)),
        KeyTarget::Grid(slot) => {
            state.commit_binding(ButtonId::Grid(*slot), action.unwrap_or(Action::None))
        }
        KeyTarget::FunctionKey(trigger) => state.commit_keyboard_binding(trigger.clone(), action),
    }
}

/// The two-pane row: keyboard photo (or keypad keycaps) + an optional side
/// panel.
#[derive(IntoElement)]
struct InspectorRow {
    keyboard: AnyElement,
    panel: Option<gpui::Div>,
}

impl InspectorRow {
    fn new(keyboard: impl Into<AnyElement>) -> Self {
        Self {
            keyboard: keyboard.into(),
            panel: None,
        }
    }

    #[must_use]
    fn panel(mut self, panel: Option<gpui::Div>) -> Self {
        self.panel = panel;
        self
    }
}

impl RenderOnce for InspectorRow {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        h_flex()
            .w_full()
            .items_center()
            .justify_center()
            .child(self.keyboard)
            .when_some(self.panel, |row, panel| {
                // The panel grows in from width 0 → PANEL_W over SLIDE_MS,
                // easing in/out, always on the right as a stable inspector.
                let animated_panel = div().overflow_hidden().child(panel).with_animation(
                    "panel-slide",
                    Animation::new(std::time::Duration::from_millis(SLIDE_MS))
                        .with_easing(ease_in_out),
                    |element, delta| element.w(px(PANEL_W * delta)),
                );
                row.gap_5().child(animated_panel)
            })
    }
}

/// The keyboard photo with callout bubbles above each function key, leader
/// lines, and invisible click-targets over the real keys.
#[derive(IntoElement)]
struct KeyboardPane {
    slots: Vec<KeySlot>,
    image_path: Option<std::path::PathBuf>,
    glow: Option<(Arc<GlowGeometry>, Hsla)>,
    render_size: (f32, f32),
    selected: Option<usize>,
    hovered: Option<usize>,
    view: Entity<FunctionRowView>,
}

impl KeyboardPane {
    fn new(
        slots: Vec<KeySlot>,
        image_path: Option<std::path::PathBuf>,
        glow: Option<(Arc<GlowGeometry>, Hsla)>,
        render_size: (f32, f32),
        view: Entity<FunctionRowView>,
    ) -> Self {
        Self {
            slots,
            image_path,
            glow,
            render_size,
            selected: None,
            hovered: None,
            view,
        }
    }

    #[must_use]
    fn selected(mut self, selected: impl Into<Option<usize>>) -> Self {
        self.selected = selected.into();
        self
    }

    #[must_use]
    fn hovered(mut self, hovered: impl Into<Option<usize>>) -> Self {
        self.hovered = hovered.into();
        self
    }
}

impl RenderOnce for KeyboardPane {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (img_w, img_h) = self.render_size;
        let img_path = self.image_path;
        let view_clone = self.view;
        let selected = self.selected;
        let hovered = self.hovered;
        let pal = theme::palette(cx);

        div()
            .relative()
            .w(px(img_w))
            .h(px(CALLOUT_BAND_H + img_h))
            .child(
            div()
                .absolute()
                .top(px(CALLOUT_BAND_H))
                .left(px(0.))
                .w(px(img_w))
                .h(px(img_h))
                // The keyboard's RGB paints *behind* the render, so the opaque
                // keys occlude it and the colour only reads through the
                // inter-key gaps — same treatment as the home gallery and the
                // mouse model.
                    .when_some(self.glow, |this, (geom, color)| {
                    this.child(glow_canvas(geom, color))
                })
                    .child(image_or_fallback(img_path, img_w, img_h, &pal)),
            )
            .child(keyboard_leader_canvas(
                self.slots.clone(),
                selected,
                hovered,
                (img_w, img_h),
            ))
            .children({
                let count = self.slots.len();
                let view_for_callouts = view_clone.clone();
                self.slots.iter().cloned().map(move |slot| {
                    let highlighted = key_is_highlighted(slot.idx, selected, hovered);
                    KeyCallout {
                        slot,
                        count,
                        highlighted,
                        img_w,
                        view: view_for_callouts.clone(),
                    }
                })
            })
            // Click-targets overlay, centered on each key's marker point.
            .child(
                div()
                    .absolute()
                    .top(px(CALLOUT_BAND_H))
                    .left(px(0.))
                    .w(px(img_w))
                    .h(px(img_h))
                    .children(self.slots.into_iter().map(|slot| {
                    let highlighted = key_is_highlighted(slot.idx, selected, hovered);
                    key_click_target(slot, highlighted, (img_w, img_h), &view_clone)
                })),
            )
    }
}

/// A markerless keypad: the device photo beside keycaps drawn from the
/// device model — the nine LCD keys as the 3×3 array they physically are and
/// the two paging keys as lower caps beneath the first two columns. The
/// photo establishes identity; the keycaps are the click targets, so no
/// marker coordinates are invented and nothing pretends the device has keys
/// it lacks. Keycaps carry their binding like callout bubbles, open the same
/// panel on click, and share the selection and hover treatments.
#[derive(IntoElement)]
struct KeypadPane {
    slots: Vec<KeySlot>,
    image: Option<std::path::PathBuf>,
    photo: (f32, f32),
    selected: Option<usize>,
    hovered: Option<usize>,
    view: Entity<FunctionRowView>,
}

impl KeypadPane {
    fn new(
        slots: Vec<KeySlot>,
        image: Option<std::path::PathBuf>,
        photo: (f32, f32),
        view: Entity<FunctionRowView>,
    ) -> Self {
        Self {
            slots,
            image,
            photo,
            selected: None,
            hovered: None,
            view,
        }
    }

    #[must_use]
    fn selected(mut self, selected: impl Into<Option<usize>>) -> Self {
        self.selected = selected.into();
        self
    }

    #[must_use]
    fn hovered(mut self, hovered: impl Into<Option<usize>>) -> Self {
        self.hovered = hovered.into();
        self
    }
}

impl RenderOnce for KeypadPane {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let canvas = keypad_canvas();
        let (selected, hovered) = (self.selected, self.hovered);
        let view = self.view;
        let pal = theme::palette(cx);
        let (photo_w, photo_h) = self.photo;
        let keycaps =
            div()
                .relative()
                .w(px(canvas.0))
                .h(px(canvas.1))
                .children(self.slots.into_iter().map(move |slot| {
                    let highlighted = key_is_highlighted(slot.idx, selected, hovered);
                    KeyCap {
                        cap: (KEYCAP_W, keycap_height(&slot.target)),
                        slot,
                        highlighted,
                        canvas,
                        view: view.clone(),
                    }
                }));
        // The render keeps the device's identity; the keycaps, sized to the
        // same height, form the aligned pair that replaces photo-plus-band.
        h_flex()
            .items_center()
            .gap_6()
            .when_some(self.image, |row, path| {
                row.child(image_or_fallback(Some(path), photo_w, photo_h, &pal))
            })
            .child(keycaps)
    }
}

/// A depot-marked keypad: the device render with its own keycaps drawn on
/// the keys. The depot's markers name the physical key each cap sits on —
/// the grid keys row-major, the paging controls at their printed places —
/// so the click targets *are* the device, and the same cap, tooltip,
/// binding, and selection machinery serves on either side of the seam.
/// Nothing is invented here that the depot did not author.
#[derive(IntoElement)]
struct KeypadPhotoPane {
    slots: Vec<KeySlot>,
    image: Option<std::path::PathBuf>,
    render_size: (f32, f32),
    selected: Option<usize>,
    hovered: Option<usize>,
    view: Entity<FunctionRowView>,
}

impl KeypadPhotoPane {
    fn new(
        slots: Vec<KeySlot>,
        image: Option<std::path::PathBuf>,
        render_size: (f32, f32),
        view: Entity<FunctionRowView>,
    ) -> Self {
        Self {
            slots,
            image,
            render_size,
            selected: None,
            hovered: None,
            view,
        }
    }

    #[must_use]
    fn selected(mut self, selected: impl Into<Option<usize>>) -> Self {
        self.selected = selected.into();
        self
    }

    #[must_use]
    fn hovered(mut self, hovered: impl Into<Option<usize>>) -> Self {
        self.hovered = hovered.into();
        self
    }
}

impl RenderOnce for KeypadPhotoPane {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (img_w, img_h) = self.render_size;
        let (selected, hovered) = (self.selected, self.hovered);
        let view = self.view;
        let pal = theme::palette(cx);

        div()
            .relative()
            .w(px(img_w))
            .h(px(img_h))
            .child(image_or_fallback(self.image, img_w, img_h, &pal))
            .children(self.slots.into_iter().map(move |slot| {
                let highlighted = key_is_highlighted(slot.idx, selected, hovered);
                let cap = overlay_cap(&slot.target, (img_w, img_h));
                KeyCap {
                    slot,
                    highlighted,
                    canvas: (img_w, img_h),
                    cap,
                    view: view.clone(),
                }
            }))
    }
}

/// Cap size on the render, as fractions of the image. Calibrated against
/// the render those markers were measured on: a cap sits inside the key it
/// marks, so the render's chassis gaps stay readable between caps. Grid
/// keys occupy the larger face; the paging controls are printed shorter.
fn overlay_cap(target: &KeyTarget, render: (f32, f32)) -> (f32, f32) {
    let (w_frac, h_frac) = match target {
        KeyTarget::Control(button) if button.cid().is_some_and(is_paging_cid) => {
            (PAGING_CAP_W_FRAC, PAGING_CAP_H_FRAC)
        }
        _ => (GRID_CAP_W_FRAC, GRID_CAP_H_FRAC),
    };
    (w_frac * render.0, h_frac * render.1)
}

const GRID_CAP_W_FRAC: f32 = 0.215;
const GRID_CAP_H_FRAC: f32 = 0.155;
const PAGING_CAP_W_FRAC: f32 = 0.225;
const PAGING_CAP_H_FRAC: f32 = 0.115;

/// The overlay render size: the portrait keypad render as tall as the
/// viewport allows under the same reserve the keyboard path keeps, minus
/// the callout band this pane never draws, capped at [`KEYBOARD_W`].
fn keypad_photo_render_size(asset: Option<&ResolvedAsset>, viewport_h: f32) -> (f32, f32) {
    let Some(asset) = asset.filter(|asset| asset.png_height > 0) else {
        return FALLBACK_KEYBOARD_SIZE;
    };
    let target_h = (viewport_h - KEYS_VERTICAL_RESERVE).max(KEYBOARD_MIN_IMG_H);
    asset_dimensions_for_png(asset, target_h, KEYBOARD_W)
}

/// One keycap: face, binding line, click behavior. The keycap itself is the
/// click target — no bubble, no leader line — and it wears the same
/// selected/hover treatment as the photo path's callout bubbles. `canvas`
/// is the space `slot.point` positions it in (the keycap canvas or the
/// render); `cap` is its size in that space.
#[derive(IntoElement)]
struct KeyCap {
    slot: KeySlot,
    highlighted: bool,
    canvas: (f32, f32),
    cap: (f32, f32),
    view: Entity<FunctionRowView>,
}

impl RenderOnce for KeyCap {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (w, h) = self.canvas;
        let (cap_w, cap_h) = self.cap;
        let pal = theme::palette(cx);
        let idx = self.slot.idx;
        let glyph = keycap_glyph(&self.slot.target);
        let highlighted = self.highlighted;
        let tooltip = self.slot.label.clone();
        let legend = self.slot.label;
        let binding = self.slot.binding;
        let binding_icon = self.slot.binding_icon;
        let left = self.slot.x_frac * w - cap_w / 2.;
        let top = self.slot.y_frac * h - cap_h / 2.;
        let view_hover = self.view.clone();
        let view_click = self.view;

        div()
            .id(("keycap", idx))
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .absolute()
            .top(px(top))
            .left(px(left))
            .w(px(cap_w))
            .h(px(cap_h))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(1.))
            .rounded_md()
            .border_1()
            .border_color(if highlighted {
                rgb(ACCENT_BLUE).into()
            } else {
                pal.border
            })
            .bg(if highlighted {
                theme::accent_tint()
            } else {
                pal.control
            })
            .cursor_pointer()
            .hover(move |s| {
                s.bg(if highlighted {
                    theme::accent_tint_hover()
                } else {
                    pal.control_hover
                })
            })
            .child(
                div()
                    .max_w(px(cap_w - 8.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if highlighted {
                        rgb(ACCENT_BLUE).into()
                    } else {
                        pal.text_primary
                    })
                    .child(match glyph {
                        Some(glyph) => glyph.to_string(),
                        None => legend.to_string(),
                    }),
            )
            .child(binding_line(
                binding,
                binding_icon,
                cap_w - 8.,
                highlighted,
                &pal,
            ))
            .on_hover(move |hovered, _window, cx| {
                let next = (*hovered).then_some(idx);
                view_hover.update(cx, |v, vcx| v.set_hovered_key(next, vcx));
            })
            .on_click(move |_ev, _window, cx| {
                view_click.update(cx, |v, vcx| v.click_key(idx, vcx));
            })
    }
}

/// The keycap's face: height and glyph. The schematic canvas sizes grid caps
/// by that model; the photo overlay sizes its caps by the depot's markers.
/// Controls outside the keypad tables draw their localized name, never a
/// made-up glyph.
fn keycap_height(target: &KeyTarget) -> f32 {
    match target {
        KeyTarget::Control(button) if button.cid().is_some_and(is_paging_cid) => KEYCAP_PAGING_H,
        KeyTarget::Control(_) | KeyTarget::Grid(_) | KeyTarget::FunctionKey(_) => KEYCAP_H,
    }
}

/// The glyph drawn on a cap's face.
fn keycap_glyph(target: &KeyTarget) -> Option<&'static str> {
    match target {
        KeyTarget::Grid(slot) => Some(grid_glyph(slot.key)),
        KeyTarget::Control(button) => button.cid().and_then(paging_glyph),
        KeyTarget::FunctionKey(_) => None,
    }
}

/// Position glyphs mirroring the physical keypad. Locales differ, geometries
/// don't — and the full key name stays one tooltip away.
fn grid_glyph(key: GridKey) -> &'static str {
    match key {
        GridKey::TopLeft => "↖",
        GridKey::TopMiddle => "↑",
        GridKey::TopRight => "↗",
        GridKey::MiddleLeft => "←",
        GridKey::Middle => "●",
        GridKey::MiddleRight => "→",
        GridKey::BottomLeft => "↙",
        GridKey::BottomMiddle => "↓",
        GridKey::BottomRight => "↘",
    }
}

/// The two paging keys' printed legends.
fn paging_glyph(cid: Cid) -> Option<&'static str> {
    match cid {
        cid if cid == Cid::new(0x01a1) => Some("‹"),
        cid if cid == Cid::new(0x01a2) => Some("›"),
        _ => None,
    }
}

/// One callout bubble in the band above the keyboard.
#[derive(IntoElement)]
struct KeyCallout {
    slot: KeySlot,
    count: usize,
    highlighted: bool,
    img_w: f32,
    view: Entity<FunctionRowView>,
}

impl RenderOnce for KeyCallout {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let pal = theme::palette(cx);
        let idx = self.slot.idx;
        let left = callout_left_px(idx, self.count, self.img_w, KEY_CALLOUT_W);
        let top = callout_top_px(idx);
        let view_hover = self.view.clone();
        let view_click = self.view;
        let binding = self.slot.binding;
        let binding_icon = self.slot.binding_icon;
        let highlighted = self.highlighted;
        let full_label = self.slot.label.clone();

        v_flex()
            .id(("key-callout", idx))
            // The bubble itself truncates the key name to one line (#1687 —
            // a long translation, e.g. German's compound names, wrapped to
            // several lines and overflowed into neighbouring callouts); the
            // tooltip is where the untruncated name stays readable.
            .tooltip(move |window, cx| Tooltip::new(full_label.clone()).build(window, cx))
            .absolute()
            .top(px(top))
            .left(px(left))
            .w(px(KEY_CALLOUT_W))
            .h(px(KEY_CALLOUT_H))
            .px_1()
            .justify_center()
            .items_center()
            .gap(px(1.))
            .rounded_md()
            .border_1()
            .border_color(if highlighted {
                rgb(ACCENT_BLUE).into()
            } else {
                pal.border
            })
            .bg(if highlighted {
                theme::accent_tint()
            } else {
                pal.control
            })
            .cursor_pointer()
            .hover(move |s| {
                s.bg(if highlighted {
                    theme::accent_tint_hover()
                } else {
                    pal.control_hover
                })
            })
            .child(
                div()
                    .max_w(px(KEY_CALLOUT_W - 8.))
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_caption()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if highlighted {
                        rgb(ACCENT_BLUE).into()
                    } else {
                        pal.text_primary
                    })
                    .child(self.slot.label.clone()),
            )
            .child(binding_line(
                binding,
                binding_icon,
                KEY_CALLOUT_W - 8.,
                highlighted,
                &pal,
            ))
            .on_hover(move |hovered, _window, cx| {
                let next = (*hovered).then_some(idx);
                view_hover.update(cx, |v, vcx| v.set_hovered_key(next, vcx));
            })
            .on_click(move |_ev, _window, cx| {
                view_click.update(cx, |v, vcx| v.click_key(idx, vcx));
            })
    }
}

/// One invisible click-target over a function key. Selecting it opens the
/// panel; hover/selection draws only a subtle keycap ring on the photo.
fn key_click_target(
    slot: KeySlot,
    highlighted: bool,
    (img_w, img_h): (f32, f32),
    view: &Entity<FunctionRowView>,
) -> impl IntoElement {
    let idx = slot.idx;
    let x_frac = slot.x_frac;
    let y_frac = slot.y_frac;
    let view_hover = view.clone();
    let view_click = view.clone();
    let left = key_target_left_px(x_frac, img_w, KEY_TARGET_W);
    let top = key_target_top_px(y_frac, img_h, KEY_TARGET_H);

    div()
        .id(("key-target", idx))
        .absolute()
        .top(px(top))
        .left(px(left))
        .w(px(KEY_TARGET_W))
        .h(px(KEY_TARGET_H))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .when(highlighted, |el| {
            el.child(
                div()
                    .w_full()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .w(px(KEY_HOTSPOT_DOT))
                            .h(px(KEY_HOTSPOT_DOT))
                            .rounded_full()
                            .border_1()
                            .border_color(gpui::Hsla::from(rgb(ACCENT_BLUE)))
                            .bg(gpui::Hsla::from(rgb(ACCENT_BLUE))),
                    )
                    .rounded_full()
                    .border_1()
                    .border_color(theme::accent_tint_hover())
                    .bg(theme::accent_tint()),
            )
        })
        .on_hover(move |hovered, _window, cx| {
            let next = (*hovered).then_some(idx);
            view_hover.update(cx, |v, vcx| v.set_hovered_key(next, vcx));
        })
        .on_click(move |_ev, _window, cx| {
            view_click.update(cx, |v, vcx| v.click_key(idx, vcx));
        })
}

fn binding_label(action: Option<&Action>) -> gpui::SharedString {
    match action {
        Some(action) => localized_action_label(action),
        None => tr!("common.off"),
    }
}

fn keyboard_leader_canvas(
    slots: Vec<KeySlot>,
    selected: Option<usize>,
    hovered: Option<usize>,
    (img_w, img_h): (f32, f32),
) -> impl IntoElement {
    let guides: Vec<(usize, f32, f32)> =
        slots.iter().map(|s| (s.idx, s.x_frac, s.y_frac)).collect();
    canvas(
        move |_bounds, _, _| (guides, selected, hovered),
        move |bounds, payload, window, _app| {
            let (guides, selected, hovered) = payload;
            paint_keyboard_leaders(bounds, guides, selected, hovered, (img_w, img_h), window);
        },
    )
    .absolute()
    .inset_0()
    .w(px(img_w))
    .h(px(CALLOUT_BAND_H + img_h))
}

fn paint_keyboard_leaders(
    bounds: Bounds<gpui::Pixels>,
    guides: Vec<(usize, f32, f32)>,
    selected: Option<usize>,
    hovered: Option<usize>,
    (img_w, img_h): (f32, f32),
    window: &mut Window,
) {
    let count = guides.len();
    for (idx, x_frac, y_frac) in guides {
        let highlighted = key_is_highlighted(idx, selected, hovered);
        let key_x = x_frac * img_w;
        let key_y = CALLOUT_BAND_H + (y_frac * img_h);
        let callout_x = callout_center_x(idx, count, img_w);
        let callout_bottom = callout_top_px(idx) + KEY_CALLOUT_H;
        let start = bounds.origin + point(px(callout_x), px(callout_bottom));
        let elbow = bounds.origin + point(px(callout_x), px(CALLOUT_BAND_H - 14.));
        let end = bounds.origin + point(px(key_x), px(key_y));

        let mut path = PathBuilder::stroke(if highlighted { px(2.) } else { px(1.) });
        path.move_to(start);
        path.line_to(elbow);
        path.line_to(end);
        if let Ok(path) = path.build() {
            if highlighted {
                window.paint_path(path, rgb(ACCENT_BLUE));
            } else {
                window.paint_path(path, hsla(0., 0., 0.55, 0.35));
            }
        }
    }
}

fn next_selection_after_click(current: Option<usize>, clicked: usize) -> Option<usize> {
    (current != Some(clicked)).then_some(clicked)
}

fn key_is_highlighted(idx: usize, selected: Option<usize>, hovered: Option<usize>) -> bool {
    selected == Some(idx) || hovered == Some(idx)
}

/// Callout bubbles lay out *evenly* across the pane instead of over their
/// keys: a dense F-row (a G513 packs Esc-F12 into half the render width)
/// would otherwise stack the bubbles into an overlapping wall. The leader
/// lines fan from each bubble down to its true key position.
#[expect(
    clippy::cast_precision_loss,
    reason = "idx/count index the function row — at most a couple of dozen keys"
)]
fn callout_center_x(idx: usize, count: usize, image_w: f32) -> f32 {
    let margin = KEY_CALLOUT_W / 2.0 + 4.0;
    if count <= 1 {
        return image_w / 2.0;
    }
    margin + (idx as f32) * (image_w - 2.0 * margin) / ((count - 1) as f32)
}

fn callout_left_px(idx: usize, count: usize, image_w: f32, callout_w: f32) -> f32 {
    (callout_center_x(idx, count, image_w) - callout_w / 2.0).clamp(0.0, image_w - callout_w)
}

fn key_target_left_px(x_frac: f32, img_w: f32, target_w: f32) -> f32 {
    (x_frac * img_w - target_w / 2.0).clamp(0.0, img_w - target_w)
}

fn key_target_top_px(y_frac: f32, img_h: f32, target_h: f32) -> f32 {
    (y_frac * img_h - target_h / 2.0).clamp(0.0, img_h - target_h)
}

fn callout_top_px(idx: usize) -> f32 {
    if callout_lane_is_lower(idx) {
        KEY_CALLOUT_TOP_LOWER
    } else {
        KEY_CALLOUT_TOP_UPPER
    }
}

fn callout_lane_is_lower(idx: usize) -> bool {
    idx.is_multiple_of(2)
}

fn binding_after_pick(current: Option<&Action>, picked: Action) -> Option<Action> {
    (current != Some(&picked)).then_some(picked)
}

/// The scrollable config panel for the selected key. Lists the same action
/// catalog the mouse picker uses, plus a Power User section. Renders the rows
/// directly (no popover) in a tall card.
impl FunctionRowView {
    fn config_panel(
        &self,
        selected_idx: usize,
        slots: &[KeySlot],
        view: &Entity<Self>,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let pal = theme::palette(cx);
        let slot = &slots[selected_idx];
        let target = slot.target.clone();
        let key_name = slot.label.clone();

        // If an editor is active, render it instead of the list.
        if let Some(kind) = self.active_editor {
            return super::editors::editor_card(
                target,
                kind,
                self.text_state.clone(),
                self.workflow_draft.clone(),
                view,
                pal,
            );
        }

        let state = AppState::try_read(cx);
        let current = state.and_then(|state| bound_action(state, &target));
        // A control or keypad key binds inside the open app profile, like a
        // mouse button.
        let app = match target {
            KeyTarget::Control(_) | KeyTarget::Grid(_) => {
                state.and_then(|state| state.editing_app().map(|app| profile_name(state, app)))
            }
            KeyTarget::FunctionKey(_) => None,
        };

        let view_for_pick = view.clone();
        let target_for_pick = target.clone();
        let current_for_pick = current.clone();
        let on_pick: PickFn = Rc::new(move |action, _window, cx| {
            AppState::apply(cx, |state| {
                commit_key_action(
                    state,
                    &target_for_pick,
                    binding_after_pick(current_for_pick.as_ref(), action),
                )
            });
            view_for_pick.update(cx, |_, vcx| vcx.notify());
        });

        let mut rows = Vec::new();
        // Unbound is not a catalog action. A control rests on its firmware
        // function (#1172); a function key rests on the key itself. The row
        // clears the override so either key returns to that native behavior.
        let view_for_native = view.clone();
        let target_for_native = target.clone();
        rows.push(
            v_flex().child(
                MenuRow::new("panel-native")
                    .selected(current.is_none())
                    .role(Role::MenuItem)
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                svg()
                                    .path("action-icons/keyboard.svg")
                                    .size_4()
                                    .flex_none()
                                    .text_color(pal.text_muted),
                            )
                            .child(div().child(tr!("keyboard.native_function"))),
                    )
                    .when(current.is_none(), |row| {
                        row.child(
                            gpui_component::Icon::new(gpui_component::IconName::Check)
                                .size_3()
                                .text_color(rgb(ACCENT_BLUE)),
                        )
                    })
                    .on_click(move |_ev, _window, cx| {
                        AppState::apply(cx, |state| {
                            commit_key_action(state, &target_for_native, None)
                        });
                        view_for_native.update(cx, |_, vcx| vcx.notify());
                    }),
            ),
        );
        rows.extend(panel_action_rows(current.as_ref(), &on_pick, view, &pal));

        compact_panel(pal)
            .w(px(PANEL_W))
            .max_h(px(500.))
            .child(title_header(&key_name, app.as_deref(), &pal))
            .child(divider(pal))
            .child(editor_scroll_list("key-panel-scroll", rows))
    }
}

/// The panel's title — which key is selected, e.g. "F1" or "Screen Capture
/// Key", and the app profile it binds in when one is open.
fn title_header(key_name: &SharedString, app: Option<&str>, pal: &Palette) -> impl IntoElement {
    let title = match app {
        Some(app) => tr!("actions.bind_control_in_app", name => key_name.clone(), app => app),
        None => tr!("actions.bind_control", name => key_name.clone()),
    };
    h_flex()
        .items_center()
        .justify_between()
        .px_2()
        .pb_1()
        .child(
            div()
                .text_caption()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(pal.text_muted)
                .child(title),
        )
}

/// The action rows + a Power User section, mirroring the picker's list but
/// adapted for the panel context (no popover dismissal).
fn panel_action_rows(
    current: Option<&Action>,
    on_pick: &PickFn,
    view: &Entity<FunctionRowView>,
    pal: &Palette,
) -> Vec<gpui::Div> {
    let mut children = action_rows("panel-action", current, on_pick, *pal);

    let power_user_actions: &[(PowerUserKind, SharedString, &'static str)] = &[
        (
            PowerUserKind::TypeText,
            tr!("actions.type_text"),
            "action-icons/keyboard.svg",
        ),
        (
            PowerUserKind::RunAppleScript,
            tr!("actions.run_applescript"),
            "action-icons/terminal.svg",
        ),
        (
            PowerUserKind::RunShellCommand,
            tr!("actions.run_shell_command"),
            "action-icons/terminal.svg",
        ),
        (
            PowerUserKind::Workflow,
            tr!("actions.workflow"),
            "action-icons/list-checks.svg",
        ),
    ];

    children.push(
        v_flex()
            .child(editor_section(tr!("actions.power_user").to_string(), *pal))
            .children(power_user_actions.iter().enumerate().map(
                |(idx, (kind, label, icon_path))| {
                    let kind = *kind;
                    let view = view.clone();
                    let selected = matches!(
                        (current, kind),
                        (Some(Action::TypeText(_)), PowerUserKind::TypeText)
                            | (
                                Some(Action::RunAppleScript(_)),
                                PowerUserKind::RunAppleScript
                            )
                            | (
                                Some(Action::RunShellCommand(_)),
                                PowerUserKind::RunShellCommand
                            )
                            | (Some(Action::Workflow(_)), PowerUserKind::Workflow)
                    );
                    MenuRow::new(format!("panel-power-{idx}"))
                        .selected(selected)
                        .role(Role::MenuItem)
                        .aria_label(label.clone())
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    svg()
                                        .path(*icon_path)
                                        .size_4()
                                        .flex_none()
                                        .text_color(pal.text_muted),
                                )
                                .child(div().child(label.clone())),
                        )
                        .when(selected, |s| {
                            s.child(
                                gpui_component::Icon::new(gpui_component::IconName::Check)
                                    .size_3()
                                    .text_color(rgb(ACCENT_BLUE)),
                            )
                        })
                        .on_click(move |_ev, _window, cx| {
                            view.update(cx, |v, vcx| v.open_editor(kind, vcx));
                        })
                },
            )),
    );
    children
}

/// The keyboard image, or a labeled placeholder when no asset resolved. The
/// element is sized to the PNG's own aspect (see [`keyboard_render_size`]), so
/// the contain-fit paints edge to edge and the marker overlays stay registered.
fn image_or_fallback(
    img_path: Option<std::path::PathBuf>,
    img_w: f32,
    img_h: f32,
    pal: &Palette,
) -> AnyElement {
    match img_path {
        Some(path) if path.exists() => img(path).w(px(img_w)).h(px(img_h)).into_any_element(),
        Some(_) | None => div()
            .w(px(img_w))
            .h(px(160.))
            .rounded_md()
            .border_1()
            .border_color(pal.border)
            .bg(pal.panel)
            .flex()
            .items_center()
            .justify_center()
            .text_color(pal.text_muted)
            .child(tr!("keyboard.no_keyboard_image_available"))
            .into_any_element(),
    }
}

#[cfg(test)]
mod tests;
