use std::time::Duration;

use notifier::Notification;

use crate::challenge;

const MINUTE: u64 = 60;
const ACCOUNT_EXISTS: &str = "Someone, probably you, just tried to create an account with this email address, \
                              but one already exists. Log in with your password instead.\n\n\
                              If this was not you, you can ignore this message: nothing was changed.";

const PASSWORD_CHANGED: &str = "The password of your account was just changed, and every device was signed out.\n\n\
                                If this was not you, reset your password now.";

pub fn account_exists(email: String) -> Notification {
    Notification { recipient: email, subject: "You already have an account".into(), body: ACCOUNT_EXISTS.into() }
}

pub fn provider_linked(email: String, provider: &str) -> Notification {
    let body = format!(
        "You can now log in to your account with {provider}: it confirmed that this email address is yours.\n\n\
         If this was not you, reset your password now."
    );
    Notification { recipient: email, subject: "A new way to log in was added to your account".into(), body }
}

pub fn password_changed(email: String) -> Notification {
    Notification { recipient: email, subject: "Your password was changed".into(), body: PASSWORD_CHANGED.into() }
}

pub fn verification_code(email: String, code: &str, validity: Duration, purpose: &str) -> Notification {
    let action = match purpose {
        challenge::LOGIN => "log in",
        challenge::PASSWORD_RESET => "reset your password",
        _ => "confirm your email address",
    };
    let body = format!(
        "Your verification code is {code}\n\nUse it to {action}. It is valid for {} minutes. \
         If you did not ask for it, ignore this message.",
        validity.as_secs() / MINUTE
    );
    Notification { recipient: email, subject: "Your Aura Seeker verification code".into(), body }
}
