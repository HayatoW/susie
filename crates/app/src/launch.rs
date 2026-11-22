//! 起動時の引数から、開くページを決める。

#[derive(Debug, PartialEq, Eq)]
pub enum StartupPage {
    Url(String),
    Fixture,
}

#[derive(Debug, PartialEq, Eq)]
pub enum StartupError {
    Switch(String),
    Extra(String),
}

impl std::fmt::Display for StartupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartupError::Switch(arg) => write!(f, "URL 以外の引数です。{arg}"),
            StartupError::Extra(arg) => write!(f, "URL は 1 つだけ渡せます。{arg}"),
        }
    }
}

impl std::error::Error for StartupError {}

pub fn startup_page<I, S>(args: I) -> Result<StartupPage, StartupError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut url = None;
    for arg in args {
        let arg = arg.into();
        if arg.starts_with('-') {
            return Err(StartupError::Switch(arg));
        }
        if url.is_some() {
            return Err(StartupError::Extra(arg));
        }
        url = Some(arg);
    }
    Ok(match url {
        Some(url) => StartupPage::Url(url),
        None => StartupPage::Fixture,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_argument_opens_the_fixture() {
        let page = startup_page(std::iter::empty::<&str>()).expect("no argument is valid");
        assert_eq!(page, StartupPage::Fixture);
    }

    #[test]
    fn one_argument_is_the_url() {
        let page = startup_page(["https://example.com"]).expect("one url is valid");
        assert_eq!(page, StartupPage::Url("https://example.com".to_string()));
    }

    #[test]
    fn second_argument_is_rejected() {
        let err = startup_page(["https://example.com", "https://example.org"])
            .expect_err("two urls are rejected");
        assert_eq!(err, StartupError::Extra("https://example.org".to_string()));
    }

    #[test]
    fn switch_is_rejected() {
        let err = startup_page(["--url"]).expect_err("a switch is rejected");
        assert_eq!(err, StartupError::Switch("--url".to_string()));
    }
}
