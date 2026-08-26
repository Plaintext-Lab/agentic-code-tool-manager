import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/svelte';
import InventoryActionDialog from '$lib/components/inventory/InventoryActionDialog.svelte';
import InventoryTable from '$lib/components/inventory/InventoryTable.svelte';
import InventoryWarningList from '$lib/components/inventory/InventoryWarningList.svelte';
import { i18n } from '$lib/i18n';
import type { InventoryRecord } from '$lib/types';

const baseRecord: InventoryRecord = {
	id: 'codex:mcp:/Users/test/.codex/config.toml:docs:0',
	client: 'codex',
	itemType: 'mcp',
	name: 'docs',
	scope: 'user',
	sourceKind: 'userConfig',
	sourcePath: '/Users/test/.codex/config.toml',
	projectPath: null,
	originalPath: '/Users/test/.codex/config.toml',
	resolvedPath: '/Users/test/.codex/config.toml',
	isSymlink: false,
	enabled: true,
	trustState: 'notApplicable',
	isEffective: true,
	sourcePriority: 100,
	protectedFields: ['Environment variables', 'HTTP headers'],
	detail: 'STDIO MCP server',
	actionCapabilities: {
		enable: { available: false, blockedReason: 'alreadyEnabled' },
		disable: { available: true, blockedReason: null },
		confirmationRequired: true,
		reloadGuidance: 'restartClient',
		sourceRevision: 'sha256:safe-fixture-revision'
	}
};

describe('InventoryTable', () => {
	it('offers the eligible Codex skill action and passes only the discovered record', async () => {
		const actions: Array<{ record: InventoryRecord; enabled: boolean }> = [];
		const skill = {
			...baseRecord,
			id: 'codex:skill:user:toggle-me',
			itemType: 'skill' as const,
			name: 'toggle-me',
			sourceKind: 'userSkills' as const,
			sourcePath: '/Users/test/.agents/skills/toggle-me/SKILL.md',
			originalPath: '/Users/test/.agents/skills/toggle-me/SKILL.md'
		};
		render(InventoryTable, {
			props: {
				records: [skill, baseRecord],
				onAction: (record: InventoryRecord, enabled: boolean) => actions.push({ record, enabled })
			}
		});

		await fireEvent.click(screen.getByRole('button', { name: 'Disable toggle-me' }));

		expect(actions).toEqual([{ record: skill, enabled: false }]);
		expect(screen.queryByRole('button', { name: 'Disable docs' })).not.toBeInTheDocument();
	});

	it('prevents a duplicate Codex skill action while its request is running', () => {
		const skill = {
			...baseRecord,
			id: 'codex:skill:user:busy',
			itemType: 'skill' as const,
			name: 'busy-skill',
			sourceKind: 'userSkills' as const
		};
		render(InventoryTable, {
			props: { records: [skill], busyRecordId: skill.id, onAction: () => undefined }
		});

		expect(screen.getByRole('button', { name: 'Disabling busy-skill' })).toBeDisabled();
	});

	it('shows tool provenance without rendering protected values', () => {
		render(InventoryTable, { props: { records: [baseRecord] } });

		expect(screen.getByText('docs')).toBeInTheDocument();
		expect(screen.getByText('Codex')).toBeInTheDocument();
		expect(screen.getByText('User config')).toBeInTheDocument();
		expect(screen.getByText('2 protected field groups hidden')).toBeInTheDocument();
		expect(document.body.textContent).not.toContain('secret-token-value');
	});

	it('shows disabled and unknown-trust states explicitly', () => {
		render(InventoryTable, {
			props: {
				records: [{ ...baseRecord, id: 'cursor:hook:1', client: 'cursor', itemType: 'hook', enabled: false, trustState: 'unknown' }]
			}
		});

		expect(screen.getByText('Disabled')).toBeInTheDocument();
		expect(screen.getByText('Trust not reported')).toBeInTheDocument();
	});

	it('shows when a project has not been trusted', () => {
		render(InventoryTable, {
			props: {
				records: [{ ...baseRecord, id: 'claude:mcp:untrusted', client: 'claude', trustState: 'untrusted', isEffective: false }]
			}
		});

		expect(screen.getByText('Project not trusted')).toBeInTheDocument();
		expect(screen.getByText('Not effective')).toBeInTheDocument();
	});

	it('labels non-project Codex hook trust separately from project trust', () => {
		render(InventoryTable, {
			props: {
				records: [
					{ ...baseRecord, id: 'codex:hook:trusted', name: 'trusted-user-hook', itemType: 'hook', trustState: 'trusted' },
					{ ...baseRecord, id: 'codex:hook:untrusted', name: 'untrusted-plugin-hook', itemType: 'hook', sourceKind: 'pluginConfig', trustState: 'untrusted', isEffective: false },
					{ ...baseRecord, id: 'claude:hook:trusted', name: 'trusted-claude-hook', client: 'claude', itemType: 'hook', trustState: 'trusted' },
					{ ...baseRecord, id: 'claude:mcp:project', name: 'untrusted-project-mcp', client: 'claude', scope: 'project', trustState: 'untrusted', isEffective: false }
				]
			}
		});

		const trustedHookRow = screen.getByText('trusted-user-hook').closest('tr');
		const untrustedHookRow = screen.getByText('untrusted-plugin-hook').closest('tr');
		const claudeHookRow = screen.getByText('trusted-claude-hook').closest('tr');
		const projectRow = screen.getByText('untrusted-project-mcp').closest('tr');

		expect(trustedHookRow).not.toBeNull();
		expect(untrustedHookRow).not.toBeNull();
		expect(claudeHookRow).not.toBeNull();
		expect(projectRow).not.toBeNull();
		expect(within(trustedHookRow!).getByText('Hook trusted')).toBeInTheDocument();
		expect(within(untrustedHookRow!).getByText('Hook not trusted')).toBeInTheDocument();
		expect(within(claudeHookRow!).getByText('Project trusted')).toBeInTheDocument();
		expect(within(projectRow!).getByText('Project not trusted')).toBeInTheDocument();
	});

	it('keeps same-named tools from separate sources visible', () => {
		render(InventoryTable, {
			props: {
				records: [baseRecord, { ...baseRecord, id: 'claude:mcp:docs:0', client: 'claude', sourcePath: '/project/.mcp.json' }]
			}
		});

		expect(screen.getAllByText('docs')).toHaveLength(2);
	});

	it('renders inventory labels in the selected language', () => {
		i18n.setLocale('zh-CN');
		try {
			render(InventoryTable, { props: { records: [baseRecord] } });
			expect(screen.getByRole('columnheader', { name: '工具' })).toBeInTheDocument();
			expect(screen.getByText('用户配置')).toBeInTheDocument();
			expect(screen.getByText('已启用')).toBeInTheDocument();
		} finally {
			i18n.setLocale('en');
		}
	});

	it('explains why records are read-only without showing protected configuration', () => {
		render(InventoryTable, {
			props: {
				records: [
					{
						...baseRecord,
						id: 'claude:mcp:managed',
						client: 'claude',
						name: 'managed-tool',
						actionCapabilities: {
							enable: { available: false, blockedReason: 'managedSource' },
							disable: { available: false, blockedReason: 'managedSource' },
							confirmationRequired: false,
							reloadGuidance: 'notRequired',
							sourceRevision: 'sha256:does-not-reveal-managed-secret'
						}
					},
					{
						...baseRecord,
						id: 'cursor:mcp:unsupported',
						client: 'cursor',
						name: 'cursor-tool',
						actionCapabilities: {
							enable: { available: false, blockedReason: 'unsupportedByClient' },
							disable: { available: false, blockedReason: 'unsupportedByClient' },
							confirmationRequired: false,
							reloadGuidance: 'notRequired',
							sourceRevision: 'sha256:does-not-reveal-cursor-secret'
						}
					}
				]
			}
		});

		expect(screen.getByText('Managed settings cannot be changed here.')).toBeInTheDocument();
		expect(screen.getByText('Cursor does not document a safe per-item control.')).toBeInTheDocument();
		expect(document.body.textContent).not.toContain('does-not-reveal-managed-secret');
		expect(document.body.textContent).not.toContain('does-not-reveal-cursor-secret');
	});

	it.each([
		['zh-CN', '托管设置无法在此处更改。'],
		['zh-TW', '受管理設定無法在此處變更。']
	] as const)('translates blocked explanations in %s', (locale, explanation) => {
		i18n.setLocale(locale);
		try {
			render(InventoryTable, {
				props: {
					records: [
						{
							...baseRecord,
							actionCapabilities: {
								enable: { available: false, blockedReason: 'managedSource' },
								disable: { available: false, blockedReason: 'managedSource' },
								confirmationRequired: false,
								reloadGuidance: 'notRequired',
								sourceRevision: 'sha256:safe-fixture-revision'
							}
						}
					]
				}
			});
			expect(screen.getByText(explanation)).toBeInTheDocument();
		} finally {
			i18n.setLocale('en');
		}
	});

	it.each([
		['en', 'Fix this broken link before changing it.'],
		['zh-CN', '请先修复此断开的链接。'],
		['zh-TW', '請先修正此中斷的連結。']
	] as const)('translates visible broken-link warnings in %s', (locale, explanation) => {
		i18n.setLocale(locale);
		try {
			render(InventoryWarningList, {
				props: {
					warnings: [
						{
							client: 'codex',
							sourcePath: '/Users/test/.agents/skills/broken-link',
							message: 'untranslated warning with protected-value',
							blockedReason: 'brokenSymlink'
						}
					]
				}
			});
			expect(screen.getByText(explanation)).toBeInTheDocument();
			expect(document.body.textContent).not.toContain('protected-value');
		} finally {
			i18n.setLocale('en');
		}
	});

	it.each([
		['en', 'The current state is unavailable.'],
		['zh-CN', '当前状态不可用。'],
		['zh-TW', '目前狀態無法取得。']
	] as const)('translates visible missing-revision warnings in %s', (locale, explanation) => {
		i18n.setLocale(locale);
		try {
			render(InventoryWarningList, {
				props: {
					warnings: [
						{
							client: 'claude',
							sourcePath: '/Users/test/.claude.json',
							message: 'untranslated revision warning',
							blockedReason: 'stateUnavailable'
						}
					]
				}
			});
			expect(screen.getByText(explanation)).toBeInTheDocument();
		} finally {
			i18n.setLocale('en');
		}
	});
});

describe('InventoryActionDialog', () => {
	it('confirms the exact client, scope, project, state, and safe source location', () => {
		const projectSkill: InventoryRecord = {
			...baseRecord,
			id: 'codex:skill:project:toggle-me',
			itemType: 'skill',
			name: 'toggle-me',
			scope: 'project',
			sourceKind: 'projectSkills',
			projectPath: '/Users/test/project',
			sourcePath: '/Users/test/project/.agents/skills/toggle-me/SKILL.md',
			originalPath: '/Users/test/project/.agents/skills/toggle-me/SKILL.md'
		};
		render(InventoryActionDialog, {
			props: {
				record: projectSkill,
				enabled: false,
				submitting: false,
				onConfirm: () => undefined,
				onCancel: () => undefined
			}
		});

		const dialog = screen.getByRole('dialog', { name: 'Disable Codex skill' });
		expect(within(dialog).getByText('toggle-me')).toBeInTheDocument();
		expect(within(dialog).getByText('Codex')).toBeInTheDocument();
		expect(within(dialog).getByText('Project scope')).toBeInTheDocument();
		expect(within(dialog).getByText('/Users/test/project')).toBeInTheDocument();
		expect(within(dialog).getByText('Disabled')).toBeInTheDocument();
		expect(within(dialog).getByText(projectSkill.sourcePath)).toBeInTheDocument();
	});

	it('disables both dialog actions while the update is running', () => {
		render(InventoryActionDialog, {
			props: {
				record: { ...baseRecord, itemType: 'skill', name: 'busy-skill' },
				enabled: false,
				submitting: true,
				onConfirm: () => undefined,
				onCancel: () => undefined
			}
		});

		expect(screen.getByRole('button', { name: 'Cancel' })).toBeDisabled();
		expect(screen.getByRole('button', { name: 'Disabling…' })).toBeDisabled();
	});
});
