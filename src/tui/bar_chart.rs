use std::{cmp::min, marker::PhantomData};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    symbols,
    widgets::{Block, Widget},
};
use unicode_width::UnicodeWidthStr;

/// ```
#[derive(Debug, Clone)]
pub struct BarChart<'a, I, S> {
    /// Block to wrap the widget in
    block: Option<Block<'a>>,
    /// The width of each bar
    bar_width: u16,
    /// The gap between each bar
    bar_gap: u16,
    /// Set of symbols used to display the data
    bar_set: symbols::bar::Set,
    /// Style of the bars
    bar_style: Style,
    /// Style of the values printed at the bottom of each bar
    value_style: Style,
    /// Style of the labels printed under each bar
    label_style: Style,
    /// Style for the widget
    style: Style,
    data: I,
    /// Value necessary for a bar to reach the maximum height (if no value is specified,
    /// the maximum value in the data is taken as reference)
    max: Option<u64>,

    _phan: PhantomData<S>,
}

#[allow(dead_code)]
impl<'a, S: AsRef<str> + 'a, I: IntoIterator<Item = &'a (S, u64)>> BarChart<'a, I, S> {
    pub fn new(data: I) -> Self {
        Self {
            block: None,
            bar_width: 1,
            bar_gap: 1,
            bar_set: symbols::bar::NINE_LEVELS,
            bar_style: Style::default(),
            value_style: Style::default(),
            label_style: Style::default(),
            style: Style::default(),
            data,
            max: None,
            _phan: PhantomData,
        }
    }
    pub fn data(mut self, data: I) -> Self {
        self.data = data;
        self
    }

    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    pub fn max(mut self, max: u64) -> Self {
        self.max = Some(max);
        self
    }

    pub fn bar_style(mut self, style: Style) -> Self {
        self.bar_style = style;
        self
    }

    pub fn bar_width(mut self, width: u16) -> Self {
        self.bar_width = width;
        self
    }

    pub fn bar_gap(mut self, gap: u16) -> Self {
        self.bar_gap = gap;
        self
    }

    pub fn bar_set(mut self, bar_set: symbols::bar::Set) -> Self {
        self.bar_set = bar_set;
        self
    }

    pub fn value_style(mut self, style: Style) -> Self {
        self.value_style = style;
        self
    }

    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }

    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

impl<'a, S: AsRef<str> + 'a, I: IntoIterator<Item = &'a (S, u64)>> Widget for BarChart<'a, I, S> {
    #[allow(clippy::cast_possible_truncation)]
    fn render(mut self, area: Rect, buf: &mut Buffer) {
        buf.set_style(area, self.style);

        let chart_area = match self.block.take() {
            Some(b) => {
                let inner_area = b.inner(area);
                b.render(area, buf);
                inner_area
            }
            None => area,
        };

        if chart_area.height < 2 {
            return;
        }

        let mut data: Vec<_> = self
            .data
            .into_iter()
            .map(|(label, value)| (label, *value))
            .collect();

        let max = match self.max {
            Some(max) => max,
            None => data.iter().map(|t| t.1).max().unwrap_or_default(),
        };

        let max_index = min(
            (chart_area.width / (self.bar_width + self.bar_gap)) as usize,
            data.len(),
        );

        data.truncate(max_index);

        for (i, (_, value)) in data.iter_mut().enumerate() {
            let mut value = *value * u64::from(chart_area.height - 1) * 8 / max.max(1);

            for j in (0..chart_area.height - 1).rev() {
                let symbol = match value {
                    0 => self.bar_set.empty,
                    1 => self.bar_set.one_eighth,
                    2 => self.bar_set.one_quarter,
                    3 => self.bar_set.three_eighths,
                    4 => self.bar_set.half,
                    5 => self.bar_set.five_eighths,
                    6 => self.bar_set.three_quarters,
                    7 => self.bar_set.seven_eighths,
                    _ => self.bar_set.full,
                };

                for x in 0..self.bar_width {
                    buf.get_mut(
                        chart_area.left() + i as u16 * (self.bar_width + self.bar_gap) + x,
                        chart_area.top() + j,
                    )
                    .set_symbol(symbol)
                    .set_style(self.bar_style);
                }

                if value > 8 {
                    value -= 8;
                } else {
                    value = 0;
                }
            }
        }

        for (i, (label, value)) in data.iter().enumerate() {
            let label = label.as_ref();
            let value_label = format!("{value}");
            let width = value_label.width() as u16;
            if width < self.bar_width {
                buf.set_string(
                    chart_area.left()
                        + i as u16 * (self.bar_width + self.bar_gap)
                        + (self.bar_width - width) / 2,
                    chart_area.bottom() - 2,
                    value_label,
                    self.value_style,
                );
            }
            buf.set_stringn(
                chart_area.left()
                    + i as u16 * (self.bar_width + self.bar_gap)
                    + (self.bar_width - label.width() as u16) / 2,
                chart_area.bottom() - 1,
                label,
                self.bar_width as usize,
                self.label_style,
            );
        }
    }
}
