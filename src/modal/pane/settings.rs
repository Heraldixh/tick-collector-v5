#![allow(dead_code)]
use crate::screen::dashboard::pane::Message;
use crate::style;

use data::chart::kline::FootprintStudy;
use data::chart::KlineChartKind;

use iced::{
    Element, Length,
    widget::{column, container, pane_grid, text},
};

pub fn kline_cfg_view<'a>(
    _study_config: &'a study::Configurator<FootprintStudy>,
    _cfg: data::chart::kline::Config,
    _kind: &'a KlineChartKind,
    _pane: pane_grid::Pane,
    _basis: data::chart::Basis,
) -> Element<'a, Message> {
    container(column![text("No settings available")])
        .width(Length::Shrink)
        .padding(28)
        .max_width(360)
        .style(style::chart_modal)
        .into()
}

pub mod study {
    use data::chart::kline::FootprintStudy;
    use iced::{widget::column, Element};

    pub trait Study: Copy + Eq {
        fn all() -> Vec<Self>;
        fn is_same_type(&self, other: &Self) -> bool;
    }

    #[derive(Debug, Clone, Copy)]
    pub enum StudyMessage<S: Study> {
        Footprint(Message<S>),
    }

    impl Study for FootprintStudy {
        fn is_same_type(&self, other: &Self) -> bool {
            std::mem::discriminant(self) == std::mem::discriminant(other)
        }

        fn all() -> Vec<Self> {
            FootprintStudy::ALL.to_vec()
        }
    }

    #[derive(Debug, Clone, Copy)]
    pub enum Message<S: Study> {
        CardToggled(S),
        StudyToggled(S, bool),
        StudyValueChanged(S),
    }

    pub enum Action<S: Study> {
        ToggleStudy(S, bool),
        ConfigureStudy(S),
    }

    pub struct Configurator<S: Study> {
        expanded_card: Option<S>,
    }

    impl<S: Study> Default for Configurator<S> {
        fn default() -> Self {
            Self {
                expanded_card: None,
            }
        }
    }

    impl<S: Study + ToString> Configurator<S> {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn update(&mut self, message: Message<S>) -> Option<Action<S>> {
            match message {
                Message::CardToggled(study) => {
                    let should_collapse = self
                        .expanded_card
                        .as_ref()
                        .is_some_and(|expanded| expanded.is_same_type(&study));

                    if should_collapse {
                        self.expanded_card = None;
                    } else {
                        self.expanded_card = Some(study);
                    }
                }
                Message::StudyToggled(study, is_checked) => {
                    return Some(Action::ToggleStudy(study, is_checked));
                }
                Message::StudyValueChanged(study) => {
                    return Some(Action::ConfigureStudy(study));
                }
            }

            None
        }

        pub fn view<'a>(
            &self,
            _active_studies: &'a [S],
            _basis: data::chart::Basis,
        ) -> Element<'a, Message<S>> {
            column![].into()
        }
    }
}
