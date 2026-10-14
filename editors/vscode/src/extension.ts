import * as path from 'path';
import * as fs from 'fs';
import * as cp from 'child_process';
import * as vscode from 'vscode';
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
    TransportKind,
    Trace
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
        traceOutputChannel: vscode.window.createOutputChannel('Shae Language Server Trace'),
    };

    client = new LanguageClient(
        'shaeLanguageServer',
        'Shae Language Server',
        serverOptions,
        clientOptions
    );

    const config = vscode.workspace.getConfiguration('shae');
    const traceStr = config.get<string>('lsp.trace.server', 'off');
    await client.setTrace(Trace.fromString(traceStr));

    try {
        await client.start();
    } catch (err) {
        vscode.window.showErrorMessage(`Failed to start Shae Language Server: ${err}`);
    }
}

class ShaeCodeLensProvider implements vscode.CodeLensProvider {
    provideCodeLenses(document: vscode.TextDocument): vscode.CodeLens[] {
        if (document.languageId !== 'shae') {
            return [];
        }
        const topOfFile = new vscode.Range(0, 0, 0, 0);
        return [
            new vscode.CodeLens(topOfFile, {
                title: '▶ Run File',
                command: 'shae.runFile',
                arguments: [document.uri]
            }),
            new vscode.CodeLens(topOfFile, {
                title: '🐞 Debug File',
                command: 'workbench.action.debug.start',
                arguments: []
            })
        ];
    }
}

async function checkToolchainVersion(context: vscode.ExtensionContext) {
    const binPath = resolveShaeBinary() || 'shae';
    cp.exec(`"${binPath}" --version`, (err, stdout) => {
        if (err) {
            vscode.window.showWarningMessage(
                'Shae toolchain executable was not found on PATH. Some features may be unavailable.',
                'Install / Update Shae'
            ).then((choice) => {
                if (choice === 'Install / Update Shae') {
                    vscode.commands.executeCommand('shae.updateBinary');
                }
            });
            return;
        }

        const match = stdout.match(/shae\s+(\d+\.\d+\.\d+)/);
        if (match) {
            const detected = match[1];
            const parts = detected.split('.').map(Number);
            // Require at least 0.3.1
            if (parts[0] === 0 && (parts[1] < 3 || (parts[1] === 3 && parts[2] < 1))) {
                vscode.window.showInformationMessage(
                    `Shae toolchain is outdated (detected v${detected}, v0.3.1+ required). Would you like to update?`,
                    'Update Now',
                    'Later'
                ).then((choice) => {
                    if (choice === 'Update Now') {
                        vscode.commands.executeCommand('shae.updateBinary');
                    }
                });
            }
        }
    });
}

export async function activate(context: vscode.ExtensionContext) {
    await startLanguageServer(context);
    checkToolchainVersion(context);

    // Register CodeLens provider
    context.subscriptions.push(
        vscode.languages.registerCodeLensProvider(
            { language: 'shae', scheme: 'file' },
            new ShaeCodeLensProvider()
        )
    );

    // Watch for trace configuration changes
    context.subscriptions.push(
        vscode.workspace.onDidChangeConfiguration(async (e) => {
            if (e.affectsConfiguration('shae.lsp.trace.server') && client) {
                const newTrace = vscode.workspace.getConfiguration('shae').get<string>('lsp.trace.server', 'off');
                await client.setTrace(Trace.fromString(newTrace));
            }
        })
    );

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

    // Register update binary command
    context.subscriptions.push(
        vscode.commands.registerCommand('shae.updateBinary', async () => {
            const isWin = process.platform === 'win32';
            const isMac = process.platform === 'darwin';
            const isArm = process.arch === 'arm64';

            let assetName = 'shae-v0.3.1-linux-x86_64.tar.gz';
            if (isWin) {
                assetName = 'shae-v0.3.1-windows-x86_64.zip';
            } else if (isMac) {
                assetName = isArm ? 'shae-v0.3.1-macos-arm64.tar.gz' : 'shae-v0.3.1-macos-x86_64.tar.gz';
            }

            const downloadUrl = `https://github.com/spdedsec/shae/releases/download/v0.3.1/${assetName}`;
            vscode.window.showInformationMessage(
                `Downloading latest Shae toolchain (${assetName})...`
            );

            // Open download in browser or run curl command
            vscode.env.openExternal(vscode.Uri.parse(downloadUrl));
        })
    );

    // Register run active file command
    context.subscriptions.push(
        vscode.commands.registerCommand('shae.runFile', async (uri?: vscode.Uri) => {
            let targetUri = uri;
            if (!targetUri) {
                const editor = vscode.window.activeTextEditor;
                if (!editor || editor.document.languageId !== 'shae') {
                    vscode.window.showWarningMessage('No active Shae file found.');
                    return;
                }
                if (editor.document.isDirty) {
                    await editor.document.save();
                }
                targetUri = editor.document.uri;
            }

            const binPath = resolveShaeBinary() || 'shae';
            const filePath = targetUri.fsPath;
            const terminal = vscode.window.terminals.find(t => t.name === 'Shae') || vscode.window.createTerminal('Shae');
            terminal.show();
            terminal.sendText(`"${binPath}" run "${filePath}"`);
        })
    );

    // Register DAP Debug Adapter Factory
    context.subscriptions.push(
        vscode.debug.registerDebugAdapterDescriptorFactory('shae', {
            createDebugAdapterDescriptor(_session: vscode.DebugSession): vscode.ProviderResult<vscode.DebugAdapterDescriptor> {
                const binPath = resolveShaeBinary() || 'shae';
                return new vscode.DebugAdapterExecutable(binPath, ['dap']);
            }
        })
    );
}

export async function deactivate(): Promise<void> {
    if (client) {
        await client.stop();
        client = undefined;
    }
}
