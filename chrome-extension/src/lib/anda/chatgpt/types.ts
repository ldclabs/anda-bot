export type ChatGptRequest =
  | { method: 'accounts' }
  | { method: 'login_start'; params: { profile_id?: string; consent?: boolean; port?: number } }
  | { method: 'login_status' | 'login_cancel'; params: { flow_id: string } }
  | { method: 'account_select' | 'logout' | 'models'; params: { profile_id: string } }
  | { method: 'model_select'; params: { profile_id: string; model: string } }
export interface ChatGptAccount {
  id: string
  label: string
  email?: string
  connected: boolean
  plan_enabled: boolean
}
export interface ChatGptAccounts {
  active?: string
  accounts: ChatGptAccount[]
  needs_setup: boolean
}
export interface ChatGptLogin {
  flow_id: string
  status: string
  authorization_url?: string
  account_id?: string
  error?: string
}
export interface ChatGptModel {
  slug: string
  display_name: string
}
