import * as fs from 'node:fs';
import * as path from 'node:path';
import * as vscode from 'vscode';
import {
	LanguageClient,
	LanguageClientOptions,
	ServerOptions,
	TransportKind,
} from 'vscode-languageclient/node';

let client: LanguageClient | undefined;

const SERVER_BINARY = process.platform === 'win32' ? 'zsh-lsp.exe' : 'zsh-lsp';

/**
 * Locate the `zsh-lsp` binary.
 *
 * The `zsh-ide.serverPath` setting wins; otherwise we look for a binary bundled
 * with the extension, then for one built in a development checkout (where the
 * extension lives at `<repo>/vscode` and cargo writes to
 * `<repo>/zsh-ls/target/<profile>`).
 */
function resolveServerPath(context: vscode.ExtensionContext): string | undefined {
	const configured = vscode.workspace.getConfiguration('zsh-ide').get<string>('serverPath');
	if (configured) {
		return configured;
	}

	const candidates = [
		context.asAbsolutePath(path.join('bin', SERVER_BINARY)),
		context.asAbsolutePath(path.join('..', 'zsh-ls', 'target', 'debug', SERVER_BINARY)),
		context.asAbsolutePath(path.join('..', 'zsh-ls', 'target', 'release', SERVER_BINARY)),
	];
	return candidates.find(candidate => fs.existsSync(candidate));
}

export async function activate(context: vscode.ExtensionContext): Promise<void> {
	const command = resolveServerPath(context);
	if (!command) {
		void vscode.window.showWarningMessage(
			`zsh-ide: no server binary found. Build it with 'cargo build' in zsh-ls/ and set "zsh-ide.serverPath" if it's somewhere else.`,
		);
		return;
	}

	// vscode-languageclient pipes the server's stderr here, one log entry per
	// line, so anything zsh-ls writes with eprintln! is readable from the
	// Output panel (or via the zsh-ide.showServerOutput command).
	const outputChannel = vscode.window.createOutputChannel('zsh-ide', { log: true });
	context.subscriptions.push(
		outputChannel,
		vscode.commands.registerCommand('zsh-ide.showServerOutput', () => outputChannel.show()),
	);
	outputChannel.info(`starting ${command}`);

	const serverOptions: ServerOptions = {
		command,
		args: [],
		transport: TransportKind.stdio,
	};

	const clientOptions: LanguageClientOptions = {
		documentSelector: [{ language: 'zsh' }],
		outputChannel,
	};

	client = new LanguageClient('zsh-ide', 'zsh-ide', serverOptions, clientOptions);
	context.subscriptions.push(client);
	await client.start();
}

export async function deactivate(): Promise<void> {
	await client?.stop();
	client = undefined;
}
