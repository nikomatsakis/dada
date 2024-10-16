use std::sync::Arc;

mod invalidated;
mod leased;
mod my;
mod our;
mod shared;
mod tenant;

use crate::interpreter::Interpreter;

#[derive(Debug)]
pub(crate) struct Permission {
    data: Arc<PermissionData>,
}

impl Permission {
    fn new(data: Arc<PermissionData>) -> Self {
        Self { data }
    }

    fn allocate(data: impl Into<PermissionData>) -> Self {
        Self::new(Arc::new(data.into()))
    }

    pub(crate) fn my(interpreter: &Interpreter<'_>) -> Self {
        Self::allocate(my::My::new(interpreter))
    }

    fn leased(interpreter: &Interpreter<'_>) -> Self {
        Self::allocate(leased::Leased::new(interpreter))
    }

    fn shared(interpreter: &Interpreter<'_>) -> Self {
        Self::allocate(shared::Shared::new(interpreter))
    }

    pub(crate) fn our(interpreter: &Interpreter<'_>) -> Self {
        Self::allocate(our::Our::new(interpreter))
    }

    /// Duplicates thie permision. Must be a non-affine permission.
    fn duplicate(&self) -> Self {
        assert!(matches!(
            &*self.data,
            PermissionData::Our(_) | PermissionData::Shared(_)
        ));

        Permission {
            data: self.data.clone(),
        }
    }

    /// True if data with this permission can be used in any way. This test does not indicate that any action
    /// has been taken by the user and hence does not alter any permissions. Actually using data
    /// requires invoking a method like [`perform_read`] which may have side-effects on other permissions;
    /// this function however indicates whether those method will succeed or return an error.
    pub(crate) fn is_valid(&self) -> bool {
        self.data.is_valid()
    }

    /// Checks that this permission permits reading of a field.
    pub(crate) fn perform_read(&self, interpreter: &Interpreter<'_>) -> eyre::Result<()> {
        self.data.perform_read(interpreter)
    }

    /// Checks that this permission permits writing to a field.
    pub(crate) fn perform_write(&self, interpreter: &Interpreter<'_>) -> eyre::Result<()> {
        self.data.perform_write(interpreter)
    }

    /// Checks that this permission permits awaiting the object.
    pub(crate) fn perform_await(&self, interpreter: &Interpreter<'_>) -> eyre::Result<()> {
        self.data.perform_await(interpreter)
    }

    /// Given `var q = p.give`, what permission does `q` get?
    ///
    /// May also affect the permissions of `p`!
    pub(crate) fn give(&self, interpreter: &Interpreter<'_>) -> eyre::Result<Permission> {
        self.data.give(self, interpreter)
    }

    /// Given `var q = p.lease`, what permission does `q` get?
    ///
    /// May also affect the permissions of `p`!
    pub(crate) fn lease(&self, interpreter: &Interpreter<'_>) -> eyre::Result<Permission> {
        self.data.lease(self, interpreter)
    }

    /// Given `var q = p.give.share`, what permission does `q` get?
    ///
    /// May also affect the permissions of `p`!
    pub(crate) fn into_share(self, interpreter: &Interpreter<'_>) -> eyre::Result<Permission> {
        self.data.into_share(interpreter)
    }

    /// Given `var q = p.share`, what permission does `q` get?
    ///
    /// May also affect the permissions of `p`!
    pub(crate) fn share(&self, interpreter: &Interpreter<'_>) -> eyre::Result<Permission> {
        self.data.share(self, interpreter)
    }
}

#[derive(Debug)]
enum PermissionData {
    My(my::My),
    Leased(leased::Leased),
    Our(our::Our),
    Shared(shared::Shared),
}

impl PermissionData {
    /// True if this is an *exclusive* permision, meaning that while it is valid, no access cannot occur through an alias.
    ///
    /// The opposite of an exclusive permission is a *shared* permision, which permit reads throug aliases.
    fn exclusive(&self) -> bool {
        match self {
            PermissionData::My(_) | PermissionData::Leased(_) => true,
            PermissionData::Our(_) | PermissionData::Shared(_) => false,
        }
    }

    /// See [`Permission::is_valid`]
    fn is_valid(&self) -> bool {
        match self {
            PermissionData::My(p) => p.is_valid(),
            PermissionData::Leased(p) => p.is_valid(),
            PermissionData::Our(p) => p.is_valid(),
            PermissionData::Shared(p) => p.is_valid(),
        }
    }

    /// See [`Permission::give`]
    fn give(&self, this: &Permission, interpreter: &Interpreter<'_>) -> eyre::Result<Permission> {
        match self {
            PermissionData::My(p) => p.give(interpreter),

            // For things that are not `my` -- i.e., either not exclusive or not owned -- then
            // giving is the same as subleasing.
            _ => self.lease(this, interpreter),
        }
    }

    /// See [`Permission::lease`]
    fn lease(&self, this: &Permission, interpreter: &Interpreter<'_>) -> eyre::Result<Permission> {
        match self {
            PermissionData::My(p) => p.lease(interpreter),
            PermissionData::Leased(p) => p.lease(interpreter),

            // For non-exclusive permisions, leasing is the same as sharing:
            PermissionData::Shared(_) | PermissionData::Our(_) => self.share(this, interpreter),
        }
    }

    /// See [`Permission::share`]
    fn into_share(self: Arc<Self>, interpreter: &Interpreter<'_>) -> eyre::Result<Permission> {
        match &*self {
            PermissionData::My(_) => Ok(Permission::our(interpreter)),
            PermissionData::Leased(p) => p.share(interpreter),
            PermissionData::Shared(_) | PermissionData::Our(_) => Ok(Permission::new(self)),
        }
    }

    /// See [`Permission::share`]
    fn share(&self, this: &Permission, interpreter: &Interpreter<'_>) -> eyre::Result<Permission> {
        match self {
            PermissionData::My(p) => p.share(interpreter),
            PermissionData::Leased(p) => p.share(interpreter),
            PermissionData::Shared(p) => p.share(this, interpreter),
            PermissionData::Our(p) => p.share(this, interpreter),
        }
    }

    /// See [`Permission::cancel`]
    fn cancel(&self, interpreter: &Interpreter<'_>) -> eyre::Result<()> {
        match self {
            PermissionData::Leased(p) => p.cancel(interpreter),
            PermissionData::Shared(p) => p.cancel(interpreter),
            PermissionData::My(_) | PermissionData::Our(_) => {
                unreachable!("cannot cancel an owned permission")
            }
        }
    }

    /// See [`Permission::check_read`]
    fn perform_read(&self, interpreter: &Interpreter<'_>) -> eyre::Result<()> {
        match self {
            PermissionData::My(p) => p.check_read(interpreter),
            PermissionData::Leased(p) => p.check_read(interpreter),
            PermissionData::Shared(p) => p.check_read(interpreter),
            PermissionData::Our(p) => p.check_read(interpreter),
        }
    }

    /// See [`Permission::check_write`]
    fn perform_write(&self, interpreter: &Interpreter<'_>) -> eyre::Result<()> {
        match self {
            PermissionData::My(p) => p.check_write(interpreter),
            PermissionData::Leased(p) => p.check_write(interpreter),
            PermissionData::Shared(p) => p.check_write(interpreter),
            PermissionData::Our(p) => p.check_write(interpreter),
        }
    }

    /// See [`Permission::check_write`]
    fn perform_await(&self, interpreter: &Interpreter<'_>) -> eyre::Result<()> {
        match self {
            PermissionData::My(p) => p.check_await(interpreter),
            PermissionData::Leased(p) => p.check_await(interpreter),
            PermissionData::Shared(p) => p.check_await(interpreter),
            PermissionData::Our(p) => p.check_await(interpreter),
        }
    }
}
