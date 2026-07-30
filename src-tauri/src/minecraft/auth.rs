use anyhow::Result;

use azalea_auth::{
    cache::ExpiringValue, get_minecraft_token, get_ms_auth_token, get_ms_link_code, get_profile,
    refresh_ms_auth_token, AccessTokenResponse, AuthError, MinecraftAuthResponse, ProfileResponse,
    XboxLiveAuth,
};
use serde::{Deserialize, Serialize};
use tracing::{error, trace};

use uuid::Uuid;

use crate::HTTP_CLIENT;

pub(crate) const AZURE_CLIENT_ID: &str = "0add8caf-2cc6-4546-b798-c3d171217dd9";
const AZURE_SCOPE: &str = "XboxLive.signin offline_access";

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MinecraftAccount {
    #[serde(rename = "Premium")]
    MsaAccount {
        msa: ExpiringValue<AccessTokenResponse>,
        xbl: ExpiringValue<XboxLiveAuth>,
        mca: ExpiringValue<MinecraftAuthResponse>,
        #[serde(flatten)]
        profile: ProfileResponse,
    },
    #[serde(rename = "Microsoft")]
    LegacyMsaAccount {
        name: String,
        uuid: Uuid,
        token: String,
        ms_auth: MsAuth,
    },
    #[serde(rename = "Offline")]
    OfflineAccount {
        name: String,
        #[serde(alias = "uuid")]
        id: Uuid,
    },
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MsAuth {
    pub expires_in: i64,
    pub access_token: String,
    pub refresh_token: String,
    #[serde(skip)]
    pub expires_after: i64,
}

impl MinecraftAccount {
    pub async fn auth_msa<F>(on_code: F) -> Result<Self, AuthError>
    where
        F: Fn(&String, &String),
    {
        let device_code =
            get_ms_link_code(&HTTP_CLIENT, Some(AZURE_CLIENT_ID), Some(AZURE_SCOPE)).await?;
        on_code(&device_code.verification_uri, &device_code.user_code);

        let msa: ExpiringValue<AccessTokenResponse> =
            get_ms_auth_token(&HTTP_CLIENT, device_code, Some(AZURE_CLIENT_ID)).await?;

        login_msa(msa).await
    }

    pub async fn auth_offline(username: String) -> Self {
        let name_str = format!("OfflinePlayer:{}", username);
        let bytes = name_str.as_bytes();
        let mut md5: [u8; 16] = md5::compute(bytes).into();

        md5[6] &= 0x0f;
        md5[6] |= 0x30;
        md5[8] &= 0x3f;
        md5[8] |= 0x80;

        let uuid = Uuid::from_bytes(md5);

        MinecraftAccount::OfflineAccount {
            name: username,
            id: uuid,
        }
    }

    pub async fn refresh(self) -> Result<MinecraftAccount> {
        match self {
            MinecraftAccount::MsaAccount {
                msa,
                xbl,
                mca,
                profile,
                ..
            } => {
                if !mca.is_expired() {
                    return Ok(MinecraftAccount::MsaAccount {
                        msa,
                        xbl,
                        mca,
                        profile,
                    });
                }

                let msa = if msa.is_expired() {
                    trace!("refreshing Microsoft auth token");
                    match refresh_ms_auth_token(
                        &HTTP_CLIENT,
                        &msa.data.refresh_token,
                        Some(AZURE_CLIENT_ID),
                        Some(AZURE_SCOPE),
                    )
                    .await
                    {
                        Ok(new_msa) => new_msa,
                        Err(e) => {
                            error!("Error refreshing Microsoft auth token: {}", e);
                            msa
                        }
                    }
                } else {
                    msa
                };

                Ok(login_msa(msa).await?)
            }
            MinecraftAccount::LegacyMsaAccount { ms_auth, .. } => {
                let msa = refresh_ms_auth_token(
                    &HTTP_CLIENT,
                    &ms_auth.refresh_token,
                    Some(AZURE_CLIENT_ID),
                    Some(AZURE_SCOPE),
                )
                .await?;
                Ok(login_msa(msa).await?)
            }
            MinecraftAccount::OfflineAccount { name, id, .. } => {
                Ok(MinecraftAccount::OfflineAccount { name, id })
            }
        }
    }

    pub async fn logout(&self) -> Result<()> {
        Ok(())
    }

    pub fn get_username(&self) -> &str {
        match self {
            MinecraftAccount::MsaAccount { profile, .. } => &profile.name,
            MinecraftAccount::LegacyMsaAccount { name, .. } => name,
            MinecraftAccount::OfflineAccount { name, .. } => name,
        }
    }
}

async fn login_msa(msa: ExpiringValue<AccessTokenResponse>) -> Result<MinecraftAccount, AuthError> {
    let msa_token = &msa.data.access_token;
    trace!("Got access token: {msa_token}");

    let minecraft = get_minecraft_token(&HTTP_CLIENT, msa_token).await?;
    let profile = get_profile(&HTTP_CLIENT, &minecraft.minecraft_access_token).await?;

    Ok(MinecraftAccount::MsaAccount {
        msa,
        xbl: minecraft.xbl,
        mca: minecraft.mca,
        profile,
    })
}
