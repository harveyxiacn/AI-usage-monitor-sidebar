// The Settings tab's sections, in page order. [FRONTEND]
//
// Shared by the tab's index and by the command palette ("Go to Settings ›
// Privacy"), so a new card shows up in both. `title` is an i18n key and `id`
// is the element id of the card.
export const SECTIONS = [
  { id: 'appearance', title: 'settings.appearance' },
  { id: 'presets', title: 'settings.card.presets' },
  { id: 'sidebarItems', title: 'settings.sidebarItems' },
  { id: 'sizeColour', title: 'settings.sizeColour' },
  { id: 'position', title: 'settings.position' },
  { id: 'behaviour', title: 'settings.behaviour' },
  { id: 'notifications', title: 'settings.card.notifications' },
  { id: 'shortcuts', title: 'settings.card.shortcuts' },
  { id: 'providers', title: 'settings.providers' },
  { id: 'accounts', title: 'settings.accounts' },
  { id: 'data', title: 'settings.data' },
  { id: 'integrations', title: 'integrations.title' },
  { id: 'updates', title: 'settings.card.updates' },
  { id: 'privacy', title: 'settings.card.privacy' },
  { id: 'backup', title: 'settings.card.backup' },
  { id: 'about', title: 'settings.about' },
] as const;
