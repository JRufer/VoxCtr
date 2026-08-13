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
  chat_url?: string;
  chat_model?: string;
  chat_api_key?: string;
  chat_system_prompt?: string;
  chat_max_history: number;
  chat_timeout_secs: number;
  chat_reply_mode: string;
  chat_reset_phrase?: string;
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
  disabled: boolean;
  openai_enabled?: boolean;
  openai_model?: string;
  openai_mode?: string;
  openai_prompt?: string;
  openai_system_prompt?: string;
}
