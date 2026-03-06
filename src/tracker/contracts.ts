/* v8 ignore file -- type-only contract surface */
/* c8 ignore file -- type-only contract surface */
import type { Issue } from '../domain/models.js'

export interface TrackerClient {
  fetchCandidates(): Promise<Issue[]>
  fetchIssuesByIds(issueIds: string[]): Promise<Issue[]>
  fetchTerminalIssues(): Promise<Issue[]>
}
