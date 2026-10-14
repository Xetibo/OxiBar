use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use iced::{
    Color, Font, Pixels, Rectangle, Size,
    advanced::{clipboard, mouse, renderer},
};
use iced_runtime::{UserInterface, user_interface};
use iced_tiny_skia::graphics::Viewport;
use oxi_auth_ui::{
    Interaction, Prompt, PromptKind,
    view::{Focus, Purpose, prompt_card_focused},
};
use oxiced::theme::{
    snapshot::{self, ThemeSnapshot},
    theme_impl,
};

struct Frame {
    pixels: Vec<u8>,
    redraw: iced::window::RedrawRequest,
}

fn frame(
    interaction: &Interaction,
    focus: Focus,
    event: iced::window::Event,
    cache: &mut user_interface::Cache,
    renderer: &mut iced::Renderer,
) -> Frame {
    let size = Size::new(700.0, 500.0);
    let mut ui = UserInterface::build(
        iced::widget::container(prompt_card_focused(
            "Private caret fixture",
            interaction,
            true,
            Purpose::Login,
            Some(focus),
        ))
        .center(iced::Fill),
        size,
        std::mem::take(cache),
        renderer,
    );
    let mut messages = Vec::new();
    let (state, _) = ui.update(
        &[iced::Event::Window(event)],
        mouse::Cursor::Unavailable,
        renderer,
        &mut clipboard::Null,
        &mut messages,
    );
    assert!(messages.is_empty());
    let user_interface::State::Updated { redraw_request, .. } = state else {
        panic!("Caret must not invalidate the authentication view");
    };
    ui.draw(
        renderer,
        &theme_impl::get_derived_iced_theme(),
        &renderer::Style {
            text_color: Color::WHITE,
        },
        mouse::Cursor::Unavailable,
    );
    let mut pixels = vec![0; 700 * 500 * 4];
    let iced::Renderer::Secondary(cpu) = renderer else {
        panic!("Private CPU renderer");
    };
    cpu.draw(
        &mut tiny_skia::PixmapMut::from_bytes(&mut pixels, 700, 500).unwrap(),
        &mut tiny_skia::Mask::new(700, 500).unwrap(),
        &Viewport::with_physical_size(Size::new(700, 500), 1.0),
        &[Rectangle::with_size(size)],
        Color::BLACK,
    );
    *cache = ui.into_cache();
    Frame {
        pixels,
        redraw: redraw_request,
    }
}

fn changed_pixels(left: &Frame, right: &Frame) -> Vec<(usize, usize)> {
    left.pixels
        .chunks_exact(4)
        .zip(right.pixels.chunks_exact(4))
        .enumerate()
        .filter_map(|(index, (left, right))| (left != right).then_some((index % 700, index / 700)))
        .collect()
}

fn deadline(frame: &Frame) -> Instant {
    let iced::window::RedrawRequest::At(at) = frame.redraw else {
        panic!("Focused caret must schedule its next blink");
    };
    at
}

#[test]
fn masked_response_caret_blinks_resets_on_edit_hides_on_blur_and_respects_reduced_motion() {
    struct Restore(Arc<ThemeSnapshot>);
    impl Drop for Restore {
        fn drop(&mut self) {
            snapshot::install(self.0.clone()).unwrap();
        }
    }
    let original = snapshot::current_snapshot();
    let _restore = Restore(original.clone());
    let mut theme = (*original).clone();
    theme.reduced_motion = false;
    snapshot::install(Arc::new(theme.clone())).unwrap();

    let mut interaction = Interaction::default();
    let attempt = interaction.begin().unwrap();
    interaction.receive(
        attempt,
        Prompt {
            kind: PromptKind::Secret,
            text: "Password".into(),
        },
    );
    let mut cache = user_interface::Cache::default();
    let mut renderer =
        iced::Renderer::Secondary(iced_tiny_skia::Renderer::new(Font::DEFAULT, Pixels(16.0)));
    let on = frame(
        &interaction,
        Focus::Input,
        iced::window::Event::RedrawRequested(Instant::now()),
        &mut cache,
        &mut renderer,
    );
    let off = frame(
        &interaction,
        Focus::Input,
        iced::window::Event::RedrawRequested(deadline(&on) + Duration::from_millis(5)),
        &mut cache,
        &mut renderer,
    );
    let line = changed_pixels(&on, &off);
    assert_eq!(
        line.len(),
        24,
        "Empty focused input must draw a one-pixel-wide caret"
    );
    assert!(line.iter().all(|point| point.0 == line[0].0));
    let again = frame(
        &interaction,
        Focus::Input,
        iced::window::Event::RedrawRequested(deadline(&off) + Duration::from_millis(5)),
        &mut cache,
        &mut renderer,
    );
    assert_eq!(on.pixels, again.pixels);

    let blurred = frame(
        &interaction,
        Focus::Input,
        iced::window::Event::Unfocused,
        &mut cache,
        &mut renderer,
    );
    assert_eq!(blurred.pixels, off.pixels);
    let refocused = frame(
        &interaction,
        Focus::Input,
        iced::window::Event::Focused,
        &mut cache,
        &mut renderer,
    );
    assert_eq!(refocused.pixels, on.pixels);

    interaction.input.push("private-fixture");
    let edited = frame(
        &interaction,
        Focus::Input,
        iced::window::Event::RedrawRequested(Instant::now()),
        &mut cache,
        &mut renderer,
    );
    let edited_off = frame(
        &interaction,
        Focus::Input,
        iced::window::Event::RedrawRequested(deadline(&edited) + Duration::from_millis(5)),
        &mut cache,
        &mut renderer,
    );
    let edited_line = changed_pixels(&edited, &edited_off);
    assert!(!edited_line.is_empty());
    assert!(edited_line.iter().all(|point| point.0 > line[0].0));
    let inactive = frame(
        &interaction,
        Focus::Submit,
        iced::window::Event::RedrawRequested(Instant::now()),
        &mut cache,
        &mut renderer,
    );
    assert_eq!(inactive.redraw, iced::window::RedrawRequest::Wait);

    theme.reduced_motion = true;
    snapshot::install(Arc::new(theme)).unwrap();
    let steady = frame(
        &interaction,
        Focus::Input,
        iced::window::Event::RedrawRequested(Instant::now()),
        &mut cache,
        &mut renderer,
    );
    let later = frame(
        &interaction,
        Focus::Input,
        iced::window::Event::RedrawRequested(Instant::now() + Duration::from_secs(2)),
        &mut cache,
        &mut renderer,
    );
    assert_eq!(steady.pixels, later.pixels);
    assert_eq!(steady.redraw, iced::window::RedrawRequest::Wait);
    assert_eq!(later.redraw, iced::window::RedrawRequest::Wait);
}
