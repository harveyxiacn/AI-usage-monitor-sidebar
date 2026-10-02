// The text "Copy diagnostics" puts on the clipboard. [FRONTEND]
//
// Everything in `Diagnostics` is already free of secrets (the backend masks
// e-mails, strips URL credentials and redacts the log); this only lays it out
// for pasting into an issue.
import type { Diagnostics } from './types';

export function formatDiagnostics(d: Diagnostics): string {
  const lines: string[] = [];
  lines.push(`AI Usage Sidebar ${d.appVersion}`);
  lines.push(`OS: ${d.os} ${d.arch}`);
  lines.push(`Display backend: ${d.backend}${d.sessionType ? ` (session: ${d.sessionType})` : ''}`);
  lines.push('');
  lines.push('Providers:');
  for (const p of d.providers) {
    const parts = [
      p.enabled ? 'enabled' : 'disabled',
      `status ${p.status ?? 'unknown'}`,
      p.loggedIn ? 'signed in' : 'not signed in',
    ];
    if (p.experimental) parts.push('experimental');
    if (p.planLabel) parts.push(`plan ${p.planLabel}`);
    if (p.account) parts.push(`account ${p.account}`);
    if (p.fetchedAt) parts.push(`fetched ${p.fetchedAt}`);
    lines.push(`- ${p.id}: ${parts.join(', ')}`);
    if (p.error) lines.push(`    error: ${p.error}`);
  }
  if (d.accounts?.length) {
    lines.push('');
    lines.push('Extra accounts:');
    for (const a of d.accounts) {
      const parts = [
        a.enabled ? 'enabled' : 'disabled',
        `status ${a.status ?? 'unknown'}`,
        `config folder ${a.configDirFound ? 'found' : 'missing'}`,
        `credentials file ${a.credentialsFileFound ? 'found' : 'missing'}`,
        `log folder ${a.logDirFound ? 'found' : 'missing'}`,
      ];
      if (a.keychainService) parts.push(`keychain item ${a.keychainService}`);
      if (a.planLabel) parts.push(`plan ${a.planLabel}`);
      if (a.account) parts.push(`account ${a.account}`);
      if (a.fetchedAt) parts.push(`fetched ${a.fetchedAt}`);
      lines.push(`- ${a.id} (${a.label}): ${parts.join(', ')}`);
      lines.push(`    config: ${a.configDir}`);
      if (a.error) lines.push(`    error: ${a.error}`);
    }
  }
  lines.push('');
  lines.push(`Log folder: ${d.logDir}`);
  lines.push(`Config folder: ${d.configDir}`);
  lines.push(`Data folder: ${d.dataDir}`);
  lines.push('');
  lines.push('Settings:');
  lines.push('```json');
  lines.push(JSON.stringify(d.settings, null, 2));
  lines.push('```');
  lines.push('');
  lines.push(`Log tail${d.logFile ? ` (${d.logFile})` : ''}:`);
  lines.push('```');
  lines.push(d.logTail || '(no log yet)');
  lines.push('```');
  return lines.join('\n');
}
