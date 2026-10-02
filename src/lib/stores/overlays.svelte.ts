// Which dashboard overlay is open: the command palette or the share-card
// preview. A plain object so the header hint, the history button and the
// palette's own shortcut can all open them. [FRONTEND]
export const overlays = $state({ palette: false, share: false });
