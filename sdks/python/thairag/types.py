from dataclasses import dataclass, field
from typing import Optional, List, Any


@dataclass
class Organization:
    id: str
    name: str
    created_at: Optional[str] = None
    updated_at: Optional[str] = None


@dataclass
class Department:
    id: str
    org_id: str
    name: str
    created_at: Optional[str] = None
    updated_at: Optional[str] = None


@dataclass
class Workspace:
    id: str
    dept_id: str
    name: str
    created_at: Optional[str] = None
    updated_at: Optional[str] = None


@dataclass
class Document:
    id: str
    workspace_id: str
    title: str
    mime_type: Optional[str] = None
    status: Optional[str] = None
    created_at: Optional[str] = None
    updated_at: Optional[str] = None


@dataclass
class ChatMessage:
    role: str
    content: str


@dataclass
class ChatChoice:
    index: int
    message: ChatMessage
    finish_reason: Optional[str] = None


@dataclass
class ChatResponse:
    id: str
    model: str
    choices: List[ChatChoice] = field(default_factory=list)
    created: Optional[int] = None
    usage: Optional[dict] = None


@dataclass
class HealthResponse:
    status: str
    version: Optional[str] = None
    providers: Optional[dict] = None


@dataclass
class SearchResult:
    query: str
    results: List[Any] = field(default_factory=list)
    total: Optional[int] = None


@dataclass
class FeedbackResponse:
    id: str
    response_id: str
    rating: int
    comment: Optional[str] = None
    created_at: Optional[str] = None
