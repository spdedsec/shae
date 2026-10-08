import * as path from 'path';
import * as fs from 'fs';
import * as cp from 'child_process';
import * as vscode from 'vscode';
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
    TransportKind
} from 'vscode-languageclient/node';

let client: LanguageClient | undefined;

function findBinaryInPath(binName: string): string | undefined {
    const isWin = process.platform === 'win32';
    const binary = isWin ? `${binName}.exe` : binName;
    const envPath = process.env.PATH || '';
    const delimiter = isWin ? ';' : ':';
    const parts = envPath.split(delimiter);

    for (const part of parts) {
        if (!part) continue;
        const full = path.join(part, binary);
        if (fs.existsSync(full)) {
            try {
                fs.accessSync(full, fs.constants.X_OK);
                return full;
            } catch {
                // Ignore if not executable
            }
        }
    }
    return undefined;
}

function resolveShaeBinary(): string | undefined {
    const config = vscode.workspace.getConfiguration('shae');
    const configuredPath = config.get<string>('lsp.path', 'shae').trim();

    // 1. Direct path check
    if (configuredPath && configuredPath !== 'shae') {
        if (fs.existsSync(configuredPath)) {
            return configuredPath;
        }
    }

    // 2. PATH lookup
    const fromPath = findBinaryInPath('shae');
    if (fromPath) {
        return fromPath;
    }

    // 3. Search in workspace target folders (release and debug)
    const folders = vscode.workspace.workspaceFolders;
    if (folders) {
        const isWin = process.platform === 'win32';
        const binName = isWin ? 'shae.exe' : 'shae';

        for (const folder of folders) {
            const root = folder.uri.fsPath;
            const candidates = [
                path.join(root, 'target', 'release', binName),
                path.join(root, 'target', 'debug', binName),
                path.join(root, '..', 'target', 'release', binName),
                path.join(root, '..', 'target', 'debug', binName),
            ];

            for (const candidate of candidates) {
                if (fs.existsSync(candidate)) {
                    return candidate;
                }
            }
        }
    }

    // Default fallback to configured string
    return configuredPath || 'shae';
}

async function startLanguageServer(context: vscode.ExtensionContext) {
    const binPath = resolveShaeBinary();
    if (!binPath) {
        vscode.window.showErrorMessage(
            'Unable to locate the "shae" executable. Please install Shae or configure "shae.lsp.path" in settings.'
        );
        return;
    }

    const serverOptions: ServerOptions = {
        run: {
            command: binPath,
            args: ['lsp'],
            transport: TransportKind.stdio,
        },
        debug: {
            command: binPath,
            args: ['lsp'],
            transport: TransportKind.stdio,
        },
    };

    const clientOptions: LanguageClientOptions = {
        documentSelector: [
            { scheme: 'file', language: 'shae' },
            { scheme: 'untitled', language: 'shae' }
        ],
        synchronize: {
            fileEvents: vscode.workspace.createFileSystemWatcher('**/*.shae'),
        },
        outputChannelName: 'Shae Language Server',
    };

    client = new LanguageClient(
        'shaeLanguageServer',
        'Shae Language Server',
        serverOptions,
        clientOptions
    );

    try {
        await client.start();
    } catch (err) {
        vscode.window.showErrorMessage(`Failed to start Shae Language Server: ${err}`);
    }
}

export async function activate(context: vscode.ExtensionContext) {
    await startLanguageServer(context);

    // Register restart command
    context.subscriptions.push(
        vscode.commands.registerCommand('shae.restartServer', async () => {
            if (client) {
                await client.stop();
                client = undefined;
            }
            await startLanguageServer(context);
            vscode.window.showInformationMessage('Shae Language Server restarted.');
        })
    );

    // Register run active file command
    context.subscriptions.push(
        vscode.commands.registerCommand('shae.runFile', () => {
            const editor = vscode.window.activeTextEditor;
            if (!editor || editor.document.languageId !== 'shae') {
                vscode.window.showWarningMessage('No active Shae file found.');
                return;
            }

            const binPath = resolveShaeBinary() || 'shae';
            const filePath = editor.document.uri.fsPath;
            const terminal = vscode.window.activeTerminal || vscode.window.createTerminal('Shae');
            terminal.show();
            terminal.sendText(`"${binPath}" "${filePath}"`);
        })
    );
}

export async function deactivate(): Promise<void> {
    if (client) {
        await client.stop();
        client = undefined;
    }
}
