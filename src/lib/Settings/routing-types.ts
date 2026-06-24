export interface TargetProcessingConfig {
  noise_suppression?: boolean;
  quiet_mode?: boolean;
  atspi_context?: boolean;
  remove_fillers?: boolean;
  spoken_punctuation?: boolean;
  auto_format_lists?: boolean;
  apply_snippets?: boolean;
  code_mode?: boolean;
}

export interface OutputTarget {
  id: string;
  label: string;
  delivery: string;
  command?: string;
  pipe_path?: string;
  socket_host?: string;
  socket_port?: number;
  socket_unix?: string;
  file_path?: string;
  file_prefix: string;
  file_timestamp: boolean;
  file_mode?: string;
  dbus_signal?: string;
  http_url?: string;
  http_method: string;
  http_headers?: Record<string, string>;
  http_json_template?: Record<string, any>;
  webhook_url?: string;
  webhook_secret?: string;
  webhook_json_template?: Record<string, any>;
  mcp_path?: string;
  mcp_tool?: string;
  mcp_args?: Record<string, any>;
  openai_prompt?: string;
  openai_model?: string;
  openai_max_tokens?: number;
  openai_timeout_secs?: number;
  send_on_release: boolean;
  append_newline: boolean;
  strip_newlines: boolean;
  initial_prompt?: string;
  processing?: TargetProcessingConfig;
  response_pipe?: string;
}

export interface HotkeyBinding {
  id: string;
  keys: string[];
  gesture: string;
  target_id: string;
  target_ids?: string[];
  tap_ms: number;
  hold_threshold_ms: number;
  label: string;
  subkey?: string;
  disabled: boolean;
  ollama_enabled?: boolean;
  ollama_model?: string;
  ollama_mode?: string;
  ollama_prompt?: string;
}
