# Security

cu is a high-privilege local automation tool. Accessibility allows it to
inspect and control applications; Screen Recording allows it to capture
screens. An agent connected to cu may be able to read or change sensitive data.

Use cu only with trusted agent clients. Prefer type --stdin --secret for
credentials, review screenshots and logs before sharing them, and do not expose
an agent endpoint beyond the local machine.

Runtime snapshots are created with owner-only permissions and expire for action
purposes after 120 seconds. Native Accessibility observations do not emit values
from editable or secure text fields. Element handles are scoped to the PID,
window ID, and snapshot that produced them; stale targets are rejected.

Please report security issues privately to the repository owner rather than
opening a public issue with exploit details.
