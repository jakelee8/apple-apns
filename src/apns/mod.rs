use std::time::Duration;

use http::header::AUTHORIZATION;
use http::{HeaderMap, HeaderValue};
use reqwest::Url;
use reqwest_middleware::ClientWithMiddleware;
use serde::Serialize;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::result::{Error, Result};

use self::header::{
    ApnsPriority, ApnsPushType, APNS_COLLAPSE_ID, APNS_EXPIRATION, APNS_ID, APNS_PRIORITY,
    APNS_PUSH_TYPE, APNS_TOPIC,
};
use self::request::{Alert, ApnsPayload, InterruptionLevel, Sound};
use self::response::ApnsResponse;

pub mod header;
pub mod request;
pub mod response;

pub const DEVELOPMENT_SERVER: &str = "https://api.sandbox.push.apple.com";
pub const PRODUCTION_SERVER: &str = "https://api.push.apple.com";

pub const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

/*
pub enum Authorization {
    /// (Required for token-based authentication) The value of this header is
    /// bearer <provider_token>, where <provider_token> is the encrypted token
    /// that authorizes you to send notifications for the specified topic. APNs
    /// ignores this header if you use certificate-based authentication. For
    /// more information, see [Establishing a Token-Based Connection to
    /// APNs](https://developer.apple.com/documentation/usernotifications/setting_up_a_remote_notification_server/establishing_a_token-based_connection_to_apns).
    Bearer(String),

    /// If you’re using certificate-based authentication, you send your provider
    /// certificate to APNs when setting up your TLS connection. For more
    /// information, see [Establishing a Certificate-Based Connection to
    /// APNs](https://developer.apple.com/documentation/usernotifications/setting_up_a_remote_notification_server/establishing_a_certificate-based_connection_to_apns).
    Certificate,
}
*/

#[derive(Debug, Default, Clone)]
pub struct ApnsClientBuilder<'a> {
    pub server: Option<&'a str>,
    pub client: Option<ClientWithMiddleware>,
    pub provider_token: Option<&'a str>,
}

impl<'a> ApnsClientBuilder<'a> {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn build(self) -> Result<ApnsClient> {
        let base_url = self.server.unwrap_or(PRODUCTION_SERVER);
        let base_url = format!("{base_url}/3/device/").parse()?;

        let client = if let Some(client) = self.client {
            client
        } else {
            let mut client = reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .pool_idle_timeout(None)
                .http2_prior_knowledge()
                .http2_keep_alive_interval(Some(Duration::from_secs(60 * 60)))
                .http2_keep_alive_timeout(Duration::from_secs(60))
                .http2_keep_alive_while_idle(true)
                // .min_tls_version(Version::TLS_1_2)
                ;

            if let Some(provider_token) = self.provider_token {
                let mut headers = HeaderMap::new();
                let mut auth_value: HeaderValue = format!("bearer {provider_token}").parse()?;
                auth_value.set_sensitive(true);
                headers.insert(AUTHORIZATION, auth_value);
                client = client.default_headers(headers);
            }

            let client = client.build()?;

            reqwest_middleware::ClientBuilder::new(client).build()
        };

        Ok(ApnsClient { base_url, client })
    }
}

#[derive(Debug, Clone)]
pub struct ApnsClient {
    base_url: Url,
    client: ClientWithMiddleware,
}

impl ApnsClient {
    pub async fn post<T>(&self, request: ApnsRequest<T>) -> Result<ApnsResponse>
    where
        T: Serialize,
    {
        let url = self.base_url.join(&request.device_token)?;
        let (headers, request): (_, ApnsPayload<T>) = request.try_into()?;

        let req = self
            .client
            .post(url)
            .headers(headers)
            .json(&request)
            .send()
            .await?;

        let response = req.json().await?;
        Ok(response)
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ApnsRequest<T> {
    /// The hex-encoded device token.
    pub device_token: String,

    /// (Required for watchOS 6 and later; recommended for macOS, iOS, tvOS, and
    /// iPadOS) The value of this header must accurately reflect the contents of
    /// your notification’s payload. If there’s a mismatch, or if the header is
    /// missing on required systems, APNs may return an error, delay the
    /// delivery of the notification, or drop it altogether.
    pub apns_push_type: ApnsPushType,

    /// A canonical UUID that is the unique ID for the notification. If an error
    /// occurs when sending the notification, APNs includes this value when
    /// reporting the error to your server. Canonical UUIDs are 32 lowercase
    /// hexadecimal digits, displayed in five groups separated by hyphens in the
    /// form 8-4-4-4-12. For example: 123e4567-e89b-12d3-a456-4266554400a0. If
    /// you omit this header, APNs creates a UUID for you and returns it in its
    /// response.
    pub apns_id: Option<Uuid>,

    /// The date at which the notification is no longer valid. This value is a
    /// UNIX epoch expressed in seconds (UTC). If the value is nonzero, APNs
    /// stores the notification and tries to deliver it at least once, repeating
    /// the attempt as needed until the specified date. If the value is 0, APNs
    /// attempts to deliver the notification only once and doesn’t store it.
    ///
    /// A single APNs attempt may involve retries over multiple network
    /// interfaces and connections of the destination device. Often these
    /// retries span over some time period, depending on the network
    /// characteristics. In addition, a push notification may take some time on
    /// the network after APNs sends it to the device. APNs uses best efforts to
    /// honor the expiry date without any guarantee. If the value is nonzero,
    /// the notification may be delivered after the mentioned date. If the value
    /// is 0, the notification may be delivered with some delay.
    pub apns_expiration: Option<OffsetDateTime>,

    /// The priority of the notification. If you omit this header, APNs sets the
    /// notification priority to 10.
    ///
    /// Specify 10 to send the notification immediately.
    ///
    /// Specify 5 to send the notification based on power considerations on the
    /// user’s device.
    ///
    /// Specify 1 to prioritize the device’s power considerations over all other
    /// factors for delivery, and prevent awakening the device.
    pub apns_priority: Option<ApnsPriority>,

    /// The topic for the notification. In general, the topic is your app’s
    /// bundle ID/app ID. It can have a suffix based on the type of push
    /// notification. If you’re using a certificate that supports PushKit VoIP
    /// or watchOS complication notifications, you must include this header with
    /// bundle ID of you app and if applicable, the proper suffix. If you’re
    /// using token-based authentication with APNs, you must include this header
    /// with the correct bundle ID and suffix combination. To learn more about
    /// app ID, see [Register an App
    /// ID](https://help.apple.com/developer-account/#/dev1b35d6f83).
    pub apns_topic: Option<String>,

    /// An identifier you use to coalesce multiple notifications into a single
    /// notification for the user. Typically, each notification request causes a
    /// new notification to be displayed on the user’s device. When sending the
    /// same notification more than once, use the same value in this header to
    /// coalesce the requests. The value of this key must not exceed 64 bytes.
    pub apns_collapse_id: Option<String>,

    /// The information for displaying an alert.
    pub alert: Option<Alert>,

    /// The number to display in a badge on your app’s icon. Specify `0` to
    /// remove the current badge, if any.
    pub badge: Option<u32>,

    /// The name of a sound file in your app’s main bundle or in the
    /// `Library/Sounds` folder of your app’s container directory or a
    /// dictionary that contains sound information for critical alerts.
    pub sound: Option<Sound>,

    /// An app-specific identifier for grouping related notifications. This
    /// value corresponds to the
    /// [`threadIdentifier`](https://developer.apple.com/documentation/usernotifications/unmutablenotificationcontent/1649872-threadidentifier)
    /// property in the `UNNotificationContent` object.
    pub thread_id: Option<String>,

    /// The notification’s type. This string must correspond to the
    /// [`identifier`](https://developer.apple.com/documentation/usernotifications/unnotificationcategory/1649276-identifier)
    /// of one of the `UNNotificationCategory` objects you register at launch
    /// time. See [Declaring Your Actionable Notification
    /// Types](https://developer.apple.com/documentation/usernotifications/declaring_your_actionable_notification_types).
    pub category: Option<String>,

    /// The background notification flag. To perform a silent background update,
    /// specify the value `1` and don’t include the `alert`, `badge`, or `sound`
    /// keys in your payload. See [Pushing Background Updates to Your
    /// App](https://developer.apple.com/documentation/usernotifications/setting_up_a_remote_notification_server/pushing_background_updates_to_your_app).
    pub content_available: Option<bool>,

    /// The notification service app extension flag. If the value is `1`, the
    /// system passes the notification to your notification service app
    /// extension before delivery. Use your extension to modify the
    /// notification’s content. See [Modifying Content in Newly Delivered
    /// Notifications](https://developer.apple.com/documentation/usernotifications/modifying_content_in_newly_delivered_notifications).
    pub mutable_content: Option<bool>,

    /// The identifier of the window brought forward. The value of this key will
    /// be populated on the
    /// [`UNNotificationContent`](https://developer.apple.com/documentation/usernotifications/unnotificationcontent)
    /// object created from the push payload. Access the value using the
    /// [`UNNotificationContent`](https://developer.apple.com/documentation/usernotifications/unnotificationcontent)
    /// object’s
    /// [`targetContentIdentifier`](https://developer.apple.com/documentation/usernotifications/unnotificationcontent/3235764-targetcontentidentifier)
    /// property.
    pub target_content_id: Option<String>,

    /// The importance and delivery timing of a notification. The string values
    /// `passive`, `active`, `time-sensitive`, or `critical` correspond to the
    /// [`UNNotificationInterruptionLevel`](https://developer.apple.com/documentation/usernotifications/unnotificationinterruptionlevel)
    /// enumeration cases.
    pub interruption_level: Option<InterruptionLevel>,

    /// The relevance score, a number between `0` and `1`, that the system uses
    /// to sort the notifications from your app. The highest score gets featured
    /// in the notification summary. See
    /// [`relevanceScore`](https://developer.apple.com/documentation/usernotifications/unnotificationcontent/3821031-relevancescore).
    pub relevance_score: Option<f64>,

    /// Additional data to send.
    pub user_info: Option<T>,
}

impl<T> TryFrom<ApnsRequest<T>> for (HeaderMap<HeaderValue>, ApnsPayload<T>)
where
    T: Serialize,
{
    type Error = Error;

    fn try_from(this: ApnsRequest<T>) -> Result<Self> {
        let mut headers = HeaderMap::new();

        let _ = headers.insert(APNS_PUSH_TYPE.clone(), this.apns_push_type.into());

        if let Some(apns_id) = this.apns_id {
            let apns_id = apns_id.hyphenated().to_string().parse()?;
            let _ = headers.insert(APNS_ID.clone(), apns_id);
        }

        if let Some(apns_expiration) = this.apns_expiration {
            let apns_expiration = apns_expiration.unix_timestamp().to_string().parse()?;
            let _ = headers.insert(APNS_EXPIRATION.clone(), apns_expiration);
        }

        if let Some(apns_priority) = this.apns_priority {
            let _ = headers.insert(APNS_PRIORITY.clone(), apns_priority.into());
        }

        if let Some(apns_topic) = this.apns_topic {
            let apns_topic = apns_topic.parse()?;
            let _ = headers.insert(APNS_TOPIC.clone(), apns_topic);
        }

        if let Some(apns_collapse_id) = this.apns_collapse_id {
            let apns_collapse_id = apns_collapse_id.parse()?;
            let _ = headers.insert(APNS_COLLAPSE_ID.clone(), apns_collapse_id);
        }

        let payload = ApnsPayload {
            alert: this.alert.map(Into::into),
            badge: this.badge,
            sound: this.sound.map(Into::into),
            thread_id: this.thread_id,
            category: this.category,
            content_available: this.content_available,
            mutable_content: this.mutable_content,
            target_content_id: this.target_content_id,
            interruption_level: this.interruption_level,
            relevance_score: this.relevance_score,
            user_info: this.user_info,
        };

        Ok((headers, payload))
    }
}
