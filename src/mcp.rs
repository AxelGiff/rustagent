
// ============================================================================
// Module MCP (Model Context Protocol)
// ----------------------------------------------------------------------------
// Ce module implémente un client MCP permettant à RustAgent de communiquer
// avec des serveurs MCP externes via le protocole JSON-RPC 2.0 sur stdio.
// ============================================================================

use std::time::Duration;
use futures_util::StreamExt;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde_json::{json, Value};
use std::fs;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use serde::{Deserialize, Serialize};

/*
#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRcpRequest {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,

}

impl JsonRcpRequest {
    pub fn new(method: &str, params: Option<Value>) -> Self {
        JsonRcpRequest {
            jsonrpc:"2.0".to_string(),
            id: uuid::Uuid::new_v4().to_string(),
            method: method.into(),
            params,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRcpError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRcpResponse {
    pub jsonrpc: String,
    pub id: Option<u64>,
    pub result: Option<Value>,
    pub error: Option<JsonRcpError>,

}
*/
/// Erreur MCP (Model Context Protocol).
///
/// Regroupe toutes les erreurs pouvant survenir lors des interactions
/// avec un serveur MCP : erreurs d'entrée/sortie (stdin/stdout), erreurs
/// de sérialisation JSON et erreurs de protocole.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    /// Erreur d'entrée/sortie (lecture/écriture sur les pipes du processus).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// Erreur de sérialisation/désérialisation JSON.
    #[error("JSON serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    /// Erreur de protocole MCP (réponse inattendue, flux fermé, etc.).
    #[error("Protocol error: {0}")]
    Protocol(String),
}

/// Informations décrivant un outil exposé par un serveur MCP.
///
/// Correspond à la structure renvoyée par la méthode `tools/list` du
/// protocole MCP. Contient le nom de l'outil, sa description et le
/// schéma JSON de ses paramètres d'entrée.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct McpToolInfo {
    /// Nom unique de l'outil (ex: "read_file").
    pub name: String,
    /// Description textuelle de l'outil (optionnelle).
    pub description: Option<String>,
    /// Schéma JSON décrivant les paramètres d'entrée de l'outil.
    /// Le champ JSON est nommé `inputSchema` dans le protocole MCP.
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// Structure de réponse pour la méthode `tools/list` du protocole MCP.
///
/// Contient la liste des outils disponibles sur le serveur MCP connecté.
#[derive(Debug, Serialize, Deserialize)]
pub struct ToolListResponse {
    /// Liste des outils exposés par le serveur MCP.
    pub tools: Vec<McpToolInfo>,
}

/// Résultat d'un appel d'outil via la méthode `tools/call` du protocole MCP.
///
/// Contient le contenu renvoyé par l'outil ainsi qu'un indicateur d'erreur.
#[derive(Debug, Deserialize)]
pub struct CallToolResult {
    /// Contenu(s) texte renvoyé(s) par l'outil.
    pub content: Vec<ToolContent>,
    /// Indique si l'outil a renvoyé une erreur (défaut : false).
    /// Le champ JSON est nommé `isError` dans le protocole MCP.
    #[serde(default)]
    #[serde(rename = "isError")]
    pub is_error: bool,
}

/// Contenu individuel renvoyé par un outil MCP.
///
/// Un outil peut renvoyer plusieurs contenus (par exemple un texte et une image).
/// Cette structure représente un seul de ces contenus.
#[derive(Debug, Deserialize)]
pub struct ToolContent {
    /// Type du contenu (ex: "text", "image", "resource").
    /// Le champ JSON est nommé `type` dans le protocole MCP.
    #[serde(rename = "type")]
    pub content_type: String,
    /// Texte du contenu (optionnel, selon le type).
    pub text: Option<String>,
}

/// Client MCP asynchrone permettant de dialoguer avec un serveur MCP.
///
/// Ce client gère la communication bidirectionnelle sur stdio avec un processus
/// serveur MCP, en utilisant le protocole JSON-RPC 2.0. Les champs sont
/// enveloppés dans des `Arc` et des `Mutex` pour un partage sécurisé.
#[derive(Clone)]
pub struct McpClient {
    /// Entrée standard (stdin) du processus serveur MCP, protégée par un mutex.
    stdin: Arc<Mutex<ChildStdin>>,
    /// Sortie standard (stdout) du processus serveur, découpée en lignes.
    stdout_lines: Arc<Mutex<Lines<BufReader<ChildStdout>>>>,
    /// Compteur atomique pour générer des identifiants de requêtes JSON-RPC uniques.
    next_id: Arc<AtomicU64>,
}

impl McpClient {
    /// Lance un processus serveur MCP et retourne un client connecté.
    ///
    /// Cette méthode asynchrone est le point d'entrée principal pour établir
    /// une connexion avec un serveur MCP externe. Elle lance le processus,
    /// redirige son stdin/stdout vers des pipes, puis effectue le handshake
    /// d'initialisation MCP obligatoire.
    ///
    /// # Arguments
    ///
    /// * `command` - La commande à exécuter pour lancer le serveur MCP
    ///   (ex: `npx`, `node`, un binaire compilé, etc.).
    /// * `args` - Les arguments à passer à la commande
    ///   (ex: `-y @modelcontextprotocol/server-filesystem /chemin`).
    ///
    /// # Retour
    ///
    /// Un `McpClient` prêt à l'emploi, ou une erreur `McpError`.
    ///
    /// # Exemple
    /// ```rust,ignore
    /// let client = McpClient::spawn("npx", &["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]).await?;
    /// ```
    pub async fn spawn(command: &str, args: &[&str]) -> Result<Self, McpError> {
        // Crée le processus serveur MCP avec les pipes pour stdin/stdout.
        // stderr est hérité pour afficher les erreurs du serveur dans la console.
        let mut child: Child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;

        // Récupère les handles stdin et stdout du processus enfant.
        let stdin = child.stdin.take().ok_or_else(|| McpError::Protocol("Failed to open stdin".into()))?;
        let stdout = child.stdout.take().ok_or_else(|| McpError::Protocol("Failed to open stdout".into()))?;

        // Enveloppe le stdout dans un BufReader pour lire ligne par ligne.
        let stdout_lines = BufReader::new(stdout).lines();

        // Construit le client avec ses champs partagés (Arc + Mutex).
        let client = Self {
            stdin: Arc::new(Mutex::new(stdin)),
            stdout_lines: Arc::new(Mutex::new(stdout_lines)),
            next_id: Arc::new(AtomicU64::new(1)),
        };

        // Le handshake MCP est obligatoire avant toute autre communication.
        client.initialize().await?;

        Ok(client)
    }

    /// Envoie une requête JSON-RPC au serveur MCP et attend la réponse correspondante.
    ///
    /// Cette méthode privée est le cœur de la communication avec le serveur MCP.
    /// Elle sérialise la requête en JSON, l'écrit sur stdin, puis lit les lignes
    /// de stdout jusqu'à trouver la réponse dont l'identifiant correspond à celui
    /// de la requête envoyée.
    ///
    /// # Arguments
    ///
    /// * `method` - La méthode JSON-RPC à appeler (ex: "tools/list", "tools/call", "initialize").
    /// * `params` - Les paramètres de la requête (optionnel).
    ///
    /// # Retour
    ///
    /// Le champ `result` de la réponse JSON-RPC, ou une erreur `McpError`.
    async fn request(&self, method: &str, params: Option<Value>) -> Result<Value, McpError> {
        // Génère un identifiant séquentiel unique pour cette requête.
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        // Construit le message JSON-RPC 2.0 standard.
        let req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params.unwrap_or_else(|| json!({}))
        });

        // Sérialise la requête en JSON et ajoute un retour à la ligne.
        // Le serveur MCP lit les requêtes ligne par ligne sur son stdin.
        let mut payload = serde_json::to_string(&req)?;
        payload.push('\n');

        // Écriture de la requête sur le stdin du processus serveur.
        {
            let mut stdin = self.stdin.lock().await;
            stdin.write_all(payload.as_bytes()).await?;
            stdin.flush().await?;
        }

        // Lecture de la réponse sur stdout.
        // On verrouille le lecteur de lignes et on parcourt les lignes
        // jusqu'à trouver celle qui correspond à notre identifiant de requête.
        let mut stdout_lines = self.stdout_lines.lock().await;
        while let Some(line) = stdout_lines.next_line().await? {
            // Ignore les lignes vides qui ne contiennent pas de message.
            if line.trim().is_empty() {
                continue;
            }

            // Tente de désérialiser la ligne en JSON.
            if let Ok(res) = serde_json::from_str::<Value>(&line) {
                // Vérifie que la réponse correspond bien à notre identifiant.
                // Les notifications (sans id) sont ignorées ici.
                if res.get("id").and_then(|v| v.as_u64()) == Some(id) {
                    // Si le serveur a renvoyé une erreur JSON-RPC, on la propage.
                    if let Some(err) = res.get("error") {
                        return Err(McpError::Protocol(err.to_string()));
                    }
                    // Retourne le champ `result`, ou Null s'il est absent.
                    return Ok(res.get("result").cloned().unwrap_or(Value::Null));
                }
            }
        }

        // Si on sort de la boucle sans réponse, c'est que le flux est fermé.
        Err(McpError::Protocol("Process stream closed without response".into()))
    }

    /// Envoie une notification JSON-RPC au serveur MCP sans attendre de réponse.
    ///
    /// Contrairement à `request`, une notification ne contient pas d'identifiant
    /// et le serveur ne renvoie pas de réponse. Elle est utilisée pour des
    /// événements unidirectionnels comme `notifications/initialized`.
    ///
    /// # Arguments
    ///
    /// * `method` - La méthode de notification JSON-RPC.
    /// * `params` - Les paramètres de la notification (optionnel).
    ///
    /// # Retour
    ///
    /// Un `Result` indiquant si l'envoi a réussi.
    async fn notify(&self, method: &str, params: Option<Value>) -> Result<(), McpError> {
        // Construit le message de notification JSON-RPC (sans champ `id`).
        let req = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params.unwrap_or_else(|| json!({}))
        });

        // Sérialise et ajoute un retour à la ligne (format ligne par ligne).
        let mut payload = serde_json::to_string(&req)?;
        payload.push('\n');

        // Écrit la notification sur le stdin du serveur.
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(payload.as_bytes()).await?;
        stdin.flush().await?;
        Ok(())
    }

    /// Effectue le handshake d'initialisation standard du protocole MCP.
    ///
    /// Cette méthode privée est appelée automatiquement lors du `spawn` du client.
    /// Elle envoie la requête `initialize` avec les informations du client et la
    /// version du protocole supportée, puis notifie le serveur que l'initialisation
    /// est terminée via `notifications/initialized`.
    ///
    /// # Retour
    ///
    /// Un `Result` indiquant si l'initialisation a réussi.
    async fn initialize(&self) -> Result<(), McpError> {
        // Envoie la requête d'initialisation avec la version du protocole,
        // les capacités du client et les informations d'identification.
        let _ = self.request("initialize", Some(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "RustAgent",
                "version": "0.1.0"
            }
        }))).await?;

        // Notifie le serveur que l'initialisation du client est terminée.
        self.notify("notifications/initialized", None).await?;
        Ok(())
    }

    /// Récupère la liste des outils exposés par le serveur MCP.
    ///
    /// Cette méthode interroge la méthode `tools/list` du protocole MCP et
    /// désérialise la réponse en une liste d'objets `McpToolInfo`.
    ///
    /// # Retour
    ///
    /// Un vecteur contenant les informations sur chaque outil exposé par
    /// le serveur MCP connecté.
    pub async fn list_tools(&self) -> Result<Vec<McpToolInfo>, McpError> {
        // Envoie la requête `tools/list` au serveur MCP.
        let res = self.request("tools/list", None).await?;

        // Désérialise la réponse JSON en structure `ToolListResponse`.
        let tool_list: ToolListResponse = serde_json::from_value(res)?;

        // Retourne uniquement le vecteur des outils.
        Ok(tool_list.tools)
    }

    /// Exécute un outil distant via la méthode `tools/call` du protocole MCP.
    ///
    /// Cette méthode envoie une requête d'appel d'outil au serveur MCP avec
    /// le nom de l'outil et ses arguments. Elle traite ensuite la réponse en
    /// extrayant le texte des contenus renvoyés.
    ///
    /// # Arguments
    ///
    /// * `name` - Le nom de l'outil à exécuter (ex: "read_file").
    /// * `arguments` - Objet JSON contenant les arguments de l'outil.
    ///
    /// # Retour
    ///
    /// Le texte concaténé des résultats renvoyés par l'outil, ou une erreur
    /// `McpError` si l'appel a échoué ou si l'outil a renvoyé une erreur.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<String, McpError> {
        // Envoie la requête d'appel d'outil avec le nom et les arguments.
        let res = self.request("tools/call", Some(json!({
            "name": name,
            "arguments": arguments
        }))).await?;

        // Désérialise la réponse en structure `CallToolResult`.
        let call_res: CallToolResult = serde_json::from_value(res)?;

        // Vérifie si l'outil a renvoyé une erreur.
        if call_res.is_error {
            return Err(McpError::Protocol("Tool returned error status".into()));
        }

        // Concatène les textes de tous les contenus renvoyés, séparés par des
        // retours à la ligne, en ignorant les contenus sans texte.
        let output = call_res.content
            .into_iter()
            .filter_map(|c| c.text)
            .collect::<Vec<_>>()
            .join("\n");

        Ok(output)
    }
}


/// Erreur d'exécution d'un outil MCP dynamique.
///
/// Cette erreur est utilisée par `McpDynamicTool` pour convertir les erreurs
/// du client MCP en erreurs compatibles avec le trait `Tool` de Rig.
#[derive(Debug, thiserror::Error)]
#[error("MCP Tool Execution Error: {0}")]
pub struct DynamicToolError(pub String);

/// Un adaptateur qui transforme un outil MCP distant en outil utilisable par Rig.
///
/// Cette structure permet d'exposer un outil MCP (provenant d'un serveur externe)
/// comme un outil natif du framework Rig, afin que le LLM puisse l'utiliser
/// de manière transparente dans ses appels d'outils.
#[derive(Clone)]
pub struct McpDynamicTool {
    /// Référence au client MCP connecté au serveur distant.
    client: McpClient,
    /// Métadonnées de l'outil distant (nom, description, schéma d'entrée).
    info: McpToolInfo,
}

impl McpDynamicTool {
    /// Crée un nouvel outil dynamique MCP.
    ///
    /// # Arguments
    ///
    /// * `client` - Le client MCP à utiliser pour exécuter l'outil distant.
    /// * `info` - Les métadonnées de l'outil (selon la réponse `tools/list`).
    ///
    /// # Retour
    ///
    /// Une instance de `McpDynamicTool` prête à être utilisée par Rig.
    pub fn new(client: McpClient, info: McpToolInfo) -> Self {
        Self { client, info }
    }
}

impl Tool for McpDynamicTool {
    // Le nom statique est requis par le trait, mais le vrai nom dynamique
    // est fourni dans `definition()` via `self.info.name`.
    const NAME: &'static str = "read_file";
    type Error = DynamicToolError;
    type Args = Value;
    type Output = String;

    /// Fournit la définition de l'outil au LLM.
    ///
    /// Cette méthode construit une `ToolDefinition` à partir des métadonnées
    /// de l'outil distant MCP afin que le LLM comprenne comment l'utiliser.
    async fn definition(&self, _prompt: String) -> ToolDefinition {
        // Log de débogage : affiche l'outil exposé au LLM.
        println!("[RIG DEFINITION] Outil exposé au LLM : {}", self.info.name);

        ToolDefinition {
            name: self.info.name.clone(),
            description: self.info.description.clone().unwrap_or_else(|| "Lit le contenu d'un fichier".to_string()),
            parameters: self.info.input_schema.clone(),
        }
    }

    /// Exécute l'outil MCP distant via le client.
    ///
    /// Cette méthode est appelée par Rig lorsque le LLM décide d'utiliser
    /// cet outil. Elle délègue l'exécution au serveur MCP via `client.call_tool`.
    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // Log de débogage : affiche l'appel d'outil reçu.
        println!("[MCP CALL REÇU DANS RIG] Appel de {} avec args: {:?}", self.info.name, args);

        // Délègue l'exécution au serveur MCP distant.
        let result = self.client
            .call_tool(&self.info.name, args)
            .await
            .map_err(|e| DynamicToolError(e.to_string()))?;

        // Log de débogage : affiche le résultat obtenu.
        println!("[MCP RÉSULTAT OBTENU] : {}", result);
        Ok(result)
    }
}