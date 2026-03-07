import type { Issue } from '../../domain/models.js'
import { createPromptBuilder } from '../prompt/index.js'

const CONTINUATION_TEMPLATE =
  'Continue working on {{ issue.identifier }} (turn {{ turn_number }} of {{ max_turns }}). Do not repeat prior setup; continue from the current thread state.'

interface BuildTurnPromptInput {
  template: string
  issue: Issue
  attempt: number | null
  turnNumber: number
  maxTurns: number
}

export async function buildTurnPrompt(input: BuildTurnPromptInput) {
  const builder = createPromptBuilder()
  if (input.turnNumber === 1) {
    return builder.build({
      template: input.template,
      issue: input.issue,
      attempt: input.attempt,
    })
  }

  return builder.build({
    template: CONTINUATION_TEMPLATE.replaceAll('{{ turn_number }}', String(input.turnNumber)).replaceAll(
      '{{ max_turns }}',
      String(input.maxTurns),
    ),
    issue: input.issue,
    attempt: input.attempt,
  })
}
