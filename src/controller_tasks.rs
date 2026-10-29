//! JoinHandle drop detaches Tokio tasks. Runtime-owned controllers must instead
//! be cancelled when their parent exits, unwinds or is aborted by the desktop.
#[derive(Default)]
pub(crate) struct OwnedControllers(Vec<tokio::task::AbortHandle>);
impl OwnedControllers {
    pub(crate) fn track(&mut self, task: &tokio::task::JoinHandle<()>) {
        self.0.push(task.abort_handle());
    }
}
impl Drop for OwnedControllers {
    fn drop(&mut self) {
        for controller in &self.0 {
            controller.abort();
        }
    }
}
