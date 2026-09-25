import {
  commands,
  ExtensionContext,
  Position,
  StatusBarAlignment,
  StatusBarItem,
  ThemeColor,
  Uri,
  window,
  workspace,
} from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  Location as ProtocolLocation,
  ServerOptions,
  State,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;
let statusBar: StatusBarItem | undefined;
let activeContext: ExtensionContext | undefined;

export async function activate(context: ExtensionContext) {
  activeContext = context;
  statusBar = window.createStatusBarItem(StatusBarAlignment.Left, 100);
  statusBar.name = "htmlang";
  statusBar.command = "htmlang.showOutput";
  context.subscriptions.push(statusBar);

  context.subscriptions.push(
    commands.registerCommand("htmlang.restartServer", restartServer),
    commands.registerCommand("htmlang.showOutput", () => {
      client?.outputChannel.show(true);
    })
  );

  await startServer(context);
}

async function startServer(context: ExtensionContext) {
  const config = workspace.getConfiguration("htmlang");
  const command = config.get<string>("server.path", "htmlang-lsp");
  const args = config.get<string[]>("server.args", []);

  const serverOptions: ServerOptions = { command, args };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "htmlang" }],
    middleware: {
      executeCommand: async (command, args, next) => {
        // The server emits `htmlang.showReferences` from code lenses; route
        // it through the built-in references viewer.
        // The server precomputes the reference locations, since a reference
        // query at the definition site would resolve the `@let` keyword.
        if (command === "htmlang.showReferences") {
          const [uri, position, locations] = args as [
            string,
            Position,
            ProtocolLocation[] | undefined,
          ];
          const target = Uri.parse(uri);
          const refs = (locations ?? []).map((l) =>
            client!.protocol2CodeConverter.asLocation(l)
          );
          await commands.executeCommand(
            "editor.action.showReferences",
            target,
            new Position(position.line, position.character),
            refs
          );
          return;
        }
        return next(command, args);
      },
    },
  };

  client = new LanguageClient(
    "htmlang",
    "htmlang Language Server",
    serverOptions,
    clientOptions
  );

  client.onDidChangeState((event) => updateStatus(event.newState));
  updateStatus(State.Starting);

  try {
    await client.start();
  } catch (err) {
    window.showErrorMessage(`htmlang: failed to start language server: ${err}`);
    updateStatus(State.Stopped);
  }
}

async function restartServer() {
  if (client) {
    try {
      await client.stop();
    } catch {
      // ignore — server might already be dead
    }
    client = undefined;
  }
  if (activeContext) {
    await startServer(activeContext);
  }
}

function updateStatus(state: State) {
  if (!statusBar) return;
  switch (state) {
    case State.Starting:
      statusBar.text = "$(sync~spin) htmlang";
      statusBar.tooltip = "htmlang language server is starting…";
      statusBar.backgroundColor = undefined;
      statusBar.show();
      break;
    case State.Running:
      statusBar.text = "$(check) htmlang";
      statusBar.tooltip = "htmlang language server is running. Click for output.";
      statusBar.backgroundColor = undefined;
      statusBar.show();
      break;
    case State.Stopped:
      statusBar.text = "$(error) htmlang";
      statusBar.tooltip =
        "htmlang language server stopped. Click for output, or run 'htmlang: Restart Language Server'.";
      statusBar.backgroundColor = new ThemeColor(
        "statusBarItem.errorBackground"
      );
      statusBar.show();
      break;
  }
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}
