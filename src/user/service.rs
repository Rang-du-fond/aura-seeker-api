use uuid::Uuid;

use super::Profile;
use crate::{
    crud::{Service, Stored},
    error::{Error, Result},
};

#[derive(Clone)]
pub struct ProfileService(pub Service<Profile>);

impl ProfileService {
    pub async fn replace(&self, requester: Uuid, id: Uuid, profile: Profile) -> Result<Stored<Profile>> {
        if requester == id {
            self.0.replace(id, profile).await
        } else {
            Err(Error::Forbidden("only the user can change their own profile"))
        }
    }
}
