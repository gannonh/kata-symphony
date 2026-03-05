import type { Issue } from '../domain/models.js'

export interface TrackerClient {
  fetchCandidates(): Promise<Issue[]>
  fetchIssuesByIds(issueIds: string[]): Promise<Issue[]>
  fetchTerminalIssues(): Promise<Issue[]>
}
