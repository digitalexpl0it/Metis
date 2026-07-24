//! Locale-aware date / number formatting.
//!
//! Uses `chrono`'s localized formatting (`unstable-locales`) keyed off the
//! active Metis locale. ICU4X can replace this later without changing call sites.

use chrono::{DateTime, Local, Locale};

use crate::locale_info;

fn chrono_locale() -> Locale {
    let info = locale_info();
    if !info.formats_from_locale {
        return Locale::en_US;
    }
    parse_chrono_locale(&info.posix).unwrap_or(Locale::en_US)
}

fn parse_chrono_locale(posix: &str) -> Option<Locale> {
    // chrono Locale variants are snake_case like en_US.
    match posix {
        "en_US" | "en" => Some(Locale::en_US),
        "en_GB" => Some(Locale::en_GB),
        "es_ES" | "es" => Some(Locale::es_ES),
        "fr_FR" | "fr" => Some(Locale::fr_FR),
        "de_DE" | "de" => Some(Locale::de_DE),
        "pt_BR" => Some(Locale::pt_BR),
        "pt_PT" | "pt" => Some(Locale::pt_PT),
        "it_IT" | "it" => Some(Locale::it_IT),
        "nl_NL" | "nl" => Some(Locale::nl_NL),
        "pl_PL" | "pl" => Some(Locale::pl_PL),
        "ru_RU" | "ru" => Some(Locale::ru_RU),
        "ja_JP" | "ja" => Some(Locale::ja_JP),
        "zh_CN" | "zh" => Some(Locale::zh_CN),
        "zh_TW" => Some(Locale::zh_TW),
        "ko_KR" | "ko" => Some(Locale::ko_KR),
        "ar_SA" | "ar" => Some(Locale::ar_SA),
        "he_IL" | "he" => Some(Locale::he_IL),
        _ => {
            // Try language-only fallbacks already covered; else None.
            let lang = posix.split('_').next()?;
            parse_chrono_locale(lang)
        }
    }
}

/// Medium date for the active locale.
pub fn format_short_date(dt: &DateTime<Local>) -> String {
    dt.format_localized("%x", chrono_locale()).to_string()
}

/// Short time for the active locale.
pub fn format_short_time(dt: &DateTime<Local>) -> String {
    dt.format_localized("%X", chrono_locale()).to_string()
}

/// Date + time.
pub fn format_short_datetime(dt: &DateTime<Local>) -> String {
    dt.format_localized("%c", chrono_locale()).to_string()
}

/// Apply a chrono strftime-style pattern with the active Metis locale
/// (weekday/month names, AM/PM, etc. when `formats_from_locale` is on).
pub fn format_pattern(dt: &DateTime<Local>, pattern: &str) -> String {
    dt.format_localized(pattern, chrono_locale()).to_string()
}

/// Format a floating number. Uses a simple locale decimal separator heuristic
/// (comma for many European locales, point otherwise).
pub fn format_decimal(value: f64, frac_digits: u8) -> String {
    let info = locale_info();
    let s = format!("{value:.prec$}", prec = frac_digits as usize);
    if !info.formats_from_locale {
        return s;
    }
    let lang = info.tag.split('_').next().unwrap_or("en");
    match lang {
        "de" | "es" | "fr" | "it" | "nl" | "pl" | "pt" | "ru" => s.replace('.', ","),
        _ => s,
    }
}
