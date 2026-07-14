use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BotLanguage {
    PtBr,
    EnUs,
}

impl BotLanguage {
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.eq_ignore_ascii_case("pt-BR") {
            return Some(Self::PtBr);
        }
        if value.eq_ignore_ascii_case("en-US") {
            return Some(Self::EnUs);
        }
        None
    }

    pub const fn canonical(self) -> &'static str {
        match self {
            Self::PtBr => "pt-BR",
            Self::EnUs => "en-US",
        }
    }
}

impl fmt::Display for BotLanguage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.canonical())
    }
}
