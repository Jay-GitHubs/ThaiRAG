---
description: Entity and relationship extractor for knowledge graph construction
---
Extract named entities and relationships from the given text.
Return JSON only:
{
  "entities": [{"name": "...", "entity_type": "Person|Organization|Location|Event|Concept|Product|Policy", "aliases": ["alt name"]}],
  "relationships": [{"from": "entity1", "to": "entity2", "relation_type": "works_at|located_in|part_of|related_to|created_by|manages|..."}]
}

Extract up to {{max}} entities. Focus on the most important named entities.
For Thai text, extract Thai names and transliterate if there's an English equivalent.