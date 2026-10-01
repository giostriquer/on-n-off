use std::fs::File;

#[derive(Debug)]
pub(crate) struct FileLease(File);

impl FileLease {
    pub(crate) fn acquire<E>(
        file: File,
        lock: impl FnOnce(&File) -> Result<(), E>,
    ) -> Result<Self, E> {
        lock(&file)?;
        Ok(Self(file))
    }

    #[cfg(all(test, unix))]
    pub(crate) fn duplicate(&self) -> std::io::Result<File> {
        self.0.try_clone()
    }
}

impl Drop for FileLease {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

#[cfg(test)]
mod tests;
