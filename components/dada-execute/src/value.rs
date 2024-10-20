use std::sync::Arc;

use dada_ir::word::Word;
use parking_lot::Mutex;

use crate::{data::Data, interpreter::Interpreter, permission::Permission};

#[derive(Debug)]
pub(crate) struct Value {
    permission: Permission,
    data: Arc<Mutex<Data>>,
}

impl Value {
    pub(crate) fn new(interpreter: &Interpreter<'_>, value: impl Into<Data>) -> Value {
        Value {
            permission: Permission::my(interpreter),
            data: Arc::new(Mutex::new(value.into())),
        }
    }
    pub(crate) fn our(interpreter: &Interpreter<'_>, value: impl Into<Data>) -> Value {
        Value {
            permission: Permission::our(interpreter),
            data: Arc::new(Mutex::new(value.into())),
        }
    }

    pub(crate) fn unit(interpreter: &Interpreter<'_>) -> Value {
        Value::new(interpreter, ())
    }

    /// Give a closure read access to the data from this value.
    ///
    /// Can fail if the permission doesn't permit reads.
    pub(crate) fn read<R>(
        &self,
        interpreter: &Interpreter<'_>,
        op: impl FnOnce(&Data) -> eyre::Result<R>,
    ) -> eyre::Result<R> {
        self.permission.perform_read(interpreter)?;
        op(&self.data.lock())
    }

    /// Give a closure write access to the data from this value.
    ///
    /// Can fail if the permission doesn't permit writes.
    pub(crate) fn write<R>(
        &self,
        interpreter: &Interpreter<'_>,
        op: impl FnOnce(&mut Data) -> eyre::Result<R>,
    ) -> eyre::Result<R> {
        self.permission.perform_write(interpreter)?;
        op(&mut self.data.lock())
    }

    pub(crate) fn field_mut<R>(
        &self,
        interpreter: &Interpreter<'_>,
        word: Word,
        op: impl FnOnce(&mut Value) -> eyre::Result<R>,
    ) -> eyre::Result<R> {
        self.permission.perform_read(interpreter)?;
        op(self.data.lock().field_mut(interpreter, word)?)
    }

    pub(crate) fn give(&mut self, interpreter: &Interpreter<'_>) -> eyre::Result<Value> {
        let permission = self.permission.give(interpreter)?;

        let data = if !self.permission.is_valid() {
            // If we gave away our permission, have to give away our data too
            std::mem::replace(&mut self.data, Arc::new(Mutex::new(Data::from(()))))
        } else {
            self.data.clone()
        };

        Ok(Value { permission, data })
    }

    pub(crate) fn into_share(self, interpreter: &Interpreter<'_>) -> eyre::Result<Value> {
        Ok(Value {
            permission: self.permission.into_share(interpreter)?,
            data: self.data,
        })
    }

    pub(crate) fn share(&self, interpreter: &Interpreter<'_>) -> eyre::Result<Value> {
        Ok(Value {
            permission: self.permission.share(interpreter)?,
            data: self.data.clone(),
        })
    }

    pub(crate) fn lease(&self, interpreter: &Interpreter<'_>) -> eyre::Result<Value> {
        Ok(Value {
            permission: self.permission.lease(interpreter)?,
            data: self.data.clone(),
        })
    }

    pub(crate) fn prepare_for_await(self, interpreter: &Interpreter) -> eyre::Result<Data> {
        self.permission.perform_await(interpreter)?;
        match Arc::try_unwrap(self.data) {
            Ok(data) => Ok(Mutex::into_inner(data)),
            Err(_) => panic!("value is owned but had multiple refs to the data"),
        }
    }
}
