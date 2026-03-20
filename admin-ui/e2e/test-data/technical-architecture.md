# NexusCommerce Platform — Technical Architecture Document

**Document ID:** ARCH-2026-001
**Version:** 3.1
**Classification:** Internal — Engineering
**Last Updated:** March 2026
**Authors:** Platform Engineering Team
**Reviewers:** CTO, VP Engineering, Principal Engineers

---

## Table of Contents

1. System Overview
2. Architecture Principles
3. Service Catalog
4. Database Design Patterns
5. API Gateway and Authentication
6. Message Queue and Event-Driven Architecture
7. Data Pipeline and Analytics
8. Deployment and CI/CD Pipeline
9. Monitoring and Observability
10. Disaster Recovery and Business Continuity
11. Scaling Strategy
12. Security Architecture
13. Infrastructure as Code
14. Appendices

---

## 1. System Overview

### 1.1 Platform Description

NexusCommerce is a cloud-native, microservices-based e-commerce platform designed to handle high-volume retail operations at global scale. The platform processes over 2 million orders per day across 45 countries, supporting both direct-to-consumer (D2C) and business-to-business (B2B) sales channels. It is built to handle peak loads during seasonal events such as Black Friday, where traffic can spike to 10 times the daily average.

The platform is composed of 10 core microservices, each responsible for a distinct business domain, communicating through a combination of synchronous REST/gRPC APIs and asynchronous event-driven messaging. The architecture follows domain-driven design (DDD) principles, with each service owning its data store and exposing well-defined interfaces.

### 1.2 High-Level Architecture Diagram

```
                            ┌──────────────────────────────────────┐
                            │           CDN (CloudFront)           │
                            └──────────────┬───────────────────────┘
                                           │
                            ┌──────────────▼───────────────────────┐
                            │     Load Balancer (AWS ALB/NLB)      │
                            └──────────────┬───────────────────────┘
                                           │
                            ┌──────────────▼───────────────────────┐
                            │        API Gateway (Kong)            │
                            │  Rate Limiting | Auth | Routing      │
                            └──────────────┬───────────────────────┘
                                           │
                ┌──────────────────────────┬┼──────────────────────────┐
                │                          ││                          │
    ┌───────────▼──────┐    ┌──────────────▼▼──────┐    ┌─────────────▼─────┐
    │  User Service    │    │   Product Service     │    │   Order Service   │
    │  (Rust/Axum)     │    │   (Rust/Axum)         │    │   (Rust/Axum)     │
    │  PostgreSQL      │    │   PostgreSQL          │    │   PostgreSQL      │
    └──────────────────┘    └───────────────────────┘    └───────────────────┘
                │                          │                          │
                │           ┌──────────────▼──────┐                   │
                │           │  Catalog Service     │                   │
                │           │  (Rust/Axum)         │                   │
                │           │  Elasticsearch       │                   │
                │           └─────────────────────┘                   │
                │                                                      │
    ┌───────────▼──────┐    ┌──────────────────────┐    ┌─────────────▼─────┐
    │  Auth Service    │    │  Payment Service      │    │  Inventory Svc    │
    │  (Rust/Axum)     │    │  (Rust/Axum)          │    │  (Rust/Axum)      │
    │  Redis           │    │  PostgreSQL           │    │  PostgreSQL       │
    └──────────────────┘    └──────────────────────┘    │  Redis            │
                                                         └───────────────────┘
    ┌──────────────────┐    ┌──────────────────────┐    ┌───────────────────┐
    │  Notification    │    │  Shipping Service     │    │  Analytics Svc    │
    │  Service         │    │  (Rust/Axum)          │    │  (Rust/Axum)      │
    │  (Rust/Axum)     │    │  PostgreSQL           │    │  ClickHouse       │
    │  SQS/SES         │    └──────────────────────┘    │  S3               │
    └──────────────────┘                                 └───────────────────┘

                            ┌──────────────────────────────────────┐
                            │     Message Bus (Apache Kafka)       │
                            │     Event Store | CQRS | Saga        │
                            └──────────────────────────────────────┘
```

### 1.3 Technology Stack Summary

| Layer | Technology | Purpose |
|-------|-----------|---------|
| **Language** | Rust | All microservices |
| **Web Framework** | Axum | HTTP/REST API servers |
| **gRPC** | tonic | Inter-service synchronous communication |
| **API Gateway** | Kong (OSS) | Routing, rate limiting, authentication |
| **Primary Database** | PostgreSQL 16 | Transactional data storage |
| **Search Engine** | Elasticsearch 8.x | Product search and catalog |
| **Cache** | Redis 7 (Cluster) | Session cache, rate limiting, inventory locks |
| **Message Broker** | Apache Kafka 3.7 | Asynchronous event streaming |
| **Object Storage** | AWS S3 | Media assets, exports, backups |
| **Analytics DB** | ClickHouse | Real-time analytics and reporting |
| **Container Runtime** | Docker | Service packaging |
| **Orchestration** | Kubernetes (EKS) | Container orchestration |
| **Service Mesh** | Istio | mTLS, traffic management, observability |
| **CI/CD** | GitHub Actions + ArgoCD | Build, test, deploy |
| **Infrastructure** | Terraform + Pulumi | Infrastructure as code |
| **Monitoring** | Prometheus + Grafana | Metrics collection and visualization |
| **Logging** | Loki + Grafana | Centralized log aggregation |
| **Tracing** | OpenTelemetry + Jaeger | Distributed request tracing |
| **DNS** | AWS Route 53 | DNS management and health checks |
| **CDN** | AWS CloudFront | Static asset delivery and edge caching |

### 1.4 Non-Functional Requirements

| Requirement | Target | Measurement |
|-------------|--------|-------------|
| Availability | 99.95% | Monthly uptime percentage |
| Response Time (p50) | < 100ms | API gateway to service response |
| Response Time (p99) | < 500ms | API gateway to service response |
| Throughput | 50,000 req/sec | Sustained peak load |
| Order Processing | 25 orders/sec | End-to-end order completion |
| Recovery Time Objective (RTO) | < 15 minutes | Time to restore service |
| Recovery Point Objective (RPO) | < 1 minute | Maximum data loss window |
| Data Retention | 7 years | Order and financial data |
| Deployment Frequency | 20+ per day | Production deployments |
| Mean Time to Recovery (MTTR) | < 30 minutes | From incident to resolution |

---

## 2. Architecture Principles

### 2.1 Core Principles

The NexusCommerce platform architecture is guided by the following principles:

**1. Domain-Driven Design**
Each service is organized around a bounded context from the business domain. Services own their data and business logic, communicating through well-defined contracts. Ubiquitous language is used consistently within each bounded context to align technical implementation with business concepts.

**2. Loose Coupling, High Cohesion**
Services are designed to be independently deployable and scalable. Dependencies between services are minimized through asynchronous communication patterns and event-driven architecture. Each service contains all the logic and data necessary to fulfill its domain responsibilities.

**3. API-First Design**
All service interfaces are designed and documented before implementation. OpenAPI 3.1 specifications serve as the contract between services and with external consumers. Breaking changes follow a versioning and deprecation policy with minimum 6-month migration windows.

**4. Infrastructure as Code**
All infrastructure is defined declaratively using Terraform and Pulumi. Manual changes to infrastructure are prohibited. All changes go through code review, automated testing, and staged rollouts.

**5. Observability by Default**
Every service emits structured logs, metrics, and distributed traces. Observability is not an afterthought but a first-class architectural concern. All services implement health checks, readiness probes, and liveness probes.

**6. Security by Design**
Security is integrated into every layer of the architecture. Zero-trust networking, encryption in transit and at rest, least-privilege access, and automated security scanning are mandatory, not optional.

**7. Resilience and Fault Tolerance**
Services are designed to handle failures gracefully. Circuit breakers, retries with exponential backoff, bulkheads, and timeouts are implemented at every service boundary. The system degrades gracefully rather than failing catastrophically.

**8. Data Sovereignty**
Each service owns its data and does not allow direct database access from other services. Data is shared only through APIs or events. This ensures that services can evolve their data models independently without breaking other services.

### 2.2 Communication Patterns

The platform uses a combination of synchronous and asynchronous communication patterns:

| Pattern | Use Case | Technology | Example |
|---------|----------|-----------|---------|
| REST API | External client requests | Axum (HTTP/JSON) | Web/mobile app to API Gateway |
| gRPC | Synchronous inter-service | tonic (HTTP/2 + Protobuf) | Order Service to Inventory Service |
| Event Streaming | Asynchronous state changes | Kafka | Order created event |
| Request-Reply | Async with response needed | Kafka (reply topics) | Payment processing result |
| CQRS | Read/write separation | Kafka + materialized views | Product catalog queries |
| Saga | Distributed transactions | Kafka (choreography) | Order fulfillment workflow |

### 2.3 Service Communication Matrix

| From \ To | User | Product | Order | Inventory | Payment | Shipping | Catalog | Notification | Auth | Analytics |
|-----------|------|---------|-------|-----------|---------|----------|---------|-------------|------|-----------|
| User | - | - | REST | - | - | - | - | Event | gRPC | Event |
| Product | - | - | - | gRPC | - | - | Event | - | - | Event |
| Order | gRPC | gRPC | - | gRPC | gRPC | Event | - | Event | - | Event |
| Inventory | - | gRPC | Event | - | - | gRPC | - | Event | - | Event |
| Payment | - | - | Event | - | - | - | - | Event | - | Event |
| Shipping | - | - | Event | gRPC | - | - | - | Event | - | Event |
| Catalog | - | gRPC | - | gRPC | - | - | - | - | - | Event |
| Auth | gRPC | - | - | - | - | - | - | Event | - | Event |

---

## 3. Service Catalog

### 3.1 User Service

**Bounded Context:** User Management and Profiles

**Responsibilities:**
- User registration, profile management, and account lifecycle
- Address book management (shipping and billing addresses)
- User preferences and notification settings
- Wishlist management
- Customer segmentation and tagging

**API Endpoints:**
```
POST   /api/v1/users                    # Register new user
GET    /api/v1/users/{id}               # Get user profile
PUT    /api/v1/users/{id}               # Update user profile
DELETE /api/v1/users/{id}               # Deactivate user account
GET    /api/v1/users/{id}/addresses     # List addresses
POST   /api/v1/users/{id}/addresses     # Add address
PUT    /api/v1/users/{id}/addresses/{addr_id}   # Update address
DELETE /api/v1/users/{id}/addresses/{addr_id}   # Delete address
GET    /api/v1/users/{id}/wishlist      # Get wishlist
POST   /api/v1/users/{id}/wishlist      # Add to wishlist
GET    /api/v1/users/{id}/preferences   # Get preferences
PUT    /api/v1/users/{id}/preferences   # Update preferences
```

**Data Store:** PostgreSQL (dedicated instance)

**Schema:**
```sql
CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email VARCHAR(255) UNIQUE NOT NULL,
    email_verified BOOLEAN DEFAULT FALSE,
    password_hash VARCHAR(255) NOT NULL,
    first_name VARCHAR(100) NOT NULL,
    last_name VARCHAR(100) NOT NULL,
    phone VARCHAR(20),
    date_of_birth DATE,
    locale VARCHAR(10) DEFAULT 'en-US',
    currency VARCHAR(3) DEFAULT 'USD',
    status VARCHAR(20) DEFAULT 'active' CHECK (status IN ('active', 'suspended', 'deactivated')),
    metadata JSONB DEFAULT '{}',
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    last_login_at TIMESTAMPTZ
);

CREATE TABLE addresses (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    label VARCHAR(50) DEFAULT 'home',
    first_name VARCHAR(100) NOT NULL,
    last_name VARCHAR(100) NOT NULL,
    street_line_1 VARCHAR(255) NOT NULL,
    street_line_2 VARCHAR(255),
    city VARCHAR(100) NOT NULL,
    state VARCHAR(100),
    postal_code VARCHAR(20) NOT NULL,
    country_code CHAR(2) NOT NULL,
    phone VARCHAR(20),
    is_default_shipping BOOLEAN DEFAULT FALSE,
    is_default_billing BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE wishlists (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    product_id UUID NOT NULL,
    added_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE(user_id, product_id)
);

CREATE INDEX idx_users_email ON users(email);
CREATE INDEX idx_users_status ON users(status);
CREATE INDEX idx_addresses_user_id ON addresses(user_id);
CREATE INDEX idx_wishlists_user_id ON wishlists(user_id);
```

**Events Published:**
- `user.registered` — When a new user account is created
- `user.updated` — When user profile information changes
- `user.deactivated` — When a user account is deactivated
- `user.address_added` — When a new address is added
- `user.login` — When a user successfully logs in

**SLA:** p99 latency < 200ms, 99.99% availability

### 3.2 Auth Service

**Bounded Context:** Authentication and Authorization

**Responsibilities:**
- JWT token issuance and validation
- OAuth 2.0 / OpenID Connect provider
- Multi-factor authentication (MFA)
- Session management
- API key management
- Rate limiting for authentication endpoints

**API Endpoints:**
```
POST   /api/v1/auth/login               # Authenticate user
POST   /api/v1/auth/logout              # Invalidate session
POST   /api/v1/auth/refresh             # Refresh access token
POST   /api/v1/auth/mfa/setup           # Initialize MFA setup
POST   /api/v1/auth/mfa/verify          # Verify MFA code
POST   /api/v1/auth/password/reset      # Request password reset
POST   /api/v1/auth/password/change     # Change password
GET    /api/v1/auth/sessions            # List active sessions
DELETE /api/v1/auth/sessions/{id}       # Revoke session
POST   /api/v1/auth/api-keys            # Create API key
GET    /api/v1/auth/api-keys            # List API keys
DELETE /api/v1/auth/api-keys/{id}       # Revoke API key
```

**Data Store:** Redis Cluster (session store, rate limiting, token blacklist)

**Token Structure:**
```json
{
  "header": {
    "alg": "RS256",
    "typ": "JWT",
    "kid": "key-2026-03"
  },
  "payload": {
    "sub": "usr_abc123",
    "iss": "https://auth.nexuscommerce.com",
    "aud": "nexuscommerce-api",
    "exp": 1711234567,
    "iat": 1711230967,
    "jti": "tok_unique_id",
    "roles": ["customer"],
    "permissions": ["read:orders", "write:orders", "read:profile", "write:profile"],
    "org_id": "org_xyz789",
    "mfa_verified": true
  }
}
```

**Key Rotation Policy:**
- RSA signing keys are rotated every 90 days
- Previous keys remain valid for token verification for 30 days after rotation
- JWKS endpoint (`/.well-known/jwks.json`) always includes current and previous keys
- Key rotation is automated via a scheduled job

### 3.3 Product Service

**Bounded Context:** Product Information Management

**Responsibilities:**
- Product CRUD operations
- Product variants (size, color, material)
- Product media (images, videos, documents)
- Pricing and tax configuration
- Product categorization and attributes
- Product bundles and kits

**API Endpoints:**
```
GET    /api/v1/products                  # List products (paginated)
POST   /api/v1/products                  # Create product
GET    /api/v1/products/{id}             # Get product details
PUT    /api/v1/products/{id}             # Update product
DELETE /api/v1/products/{id}             # Archive product
GET    /api/v1/products/{id}/variants    # List variants
POST   /api/v1/products/{id}/variants    # Create variant
PUT    /api/v1/products/{id}/variants/{vid} # Update variant
GET    /api/v1/products/{id}/media       # List media
POST   /api/v1/products/{id}/media       # Upload media
GET    /api/v1/categories                # List categories
POST   /api/v1/categories                # Create category
GET    /api/v1/products/{id}/pricing     # Get pricing rules
PUT    /api/v1/products/{id}/pricing     # Update pricing
```

**Data Store:** PostgreSQL (product data) + S3 (media assets)

**Schema (Key Tables):**
```sql
CREATE TABLE products (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    sku VARCHAR(50) UNIQUE NOT NULL,
    name VARCHAR(255) NOT NULL,
    slug VARCHAR(255) UNIQUE NOT NULL,
    description TEXT,
    short_description VARCHAR(500),
    brand VARCHAR(100),
    category_id UUID REFERENCES categories(id),
    status VARCHAR(20) DEFAULT 'draft' CHECK (status IN ('draft', 'active', 'discontinued', 'archived')),
    product_type VARCHAR(20) DEFAULT 'simple' CHECK (product_type IN ('simple', 'configurable', 'bundle', 'virtual')),
    attributes JSONB DEFAULT '{}',
    seo_title VARCHAR(255),
    seo_description VARCHAR(500),
    seo_keywords TEXT[],
    weight_grams INTEGER,
    dimensions JSONB, -- {"length": 10, "width": 5, "height": 3, "unit": "cm"}
    tax_class VARCHAR(50) DEFAULT 'standard',
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    published_at TIMESTAMPTZ
);

CREATE TABLE product_variants (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    product_id UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    sku VARCHAR(50) UNIQUE NOT NULL,
    name VARCHAR(255) NOT NULL,
    price_cents BIGINT NOT NULL,
    compare_at_price_cents BIGINT,
    cost_cents BIGINT,
    currency VARCHAR(3) DEFAULT 'USD',
    attributes JSONB DEFAULT '{}', -- {"color": "red", "size": "XL"}
    weight_grams INTEGER,
    barcode VARCHAR(50),
    is_default BOOLEAN DEFAULT FALSE,
    status VARCHAR(20) DEFAULT 'active',
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE product_media (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    product_id UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    variant_id UUID REFERENCES product_variants(id) ON DELETE SET NULL,
    media_type VARCHAR(20) NOT NULL CHECK (media_type IN ('image', 'video', 'document')),
    url VARCHAR(1024) NOT NULL,
    alt_text VARCHAR(255),
    sort_order INTEGER DEFAULT 0,
    metadata JSONB DEFAULT '{}',
    created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE categories (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    parent_id UUID REFERENCES categories(id),
    name VARCHAR(255) NOT NULL,
    slug VARCHAR(255) UNIQUE NOT NULL,
    description TEXT,
    image_url VARCHAR(1024),
    sort_order INTEGER DEFAULT 0,
    is_active BOOLEAN DEFAULT TRUE,
    path LTREE, -- Materialized path for efficient tree queries
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);
```

**Events Published:**
- `product.created` — New product created
- `product.updated` — Product information changed
- `product.published` — Product made available for sale
- `product.discontinued` — Product marked as discontinued
- `product.price_changed` — Product pricing updated
- `product.media_added` — New media attached to product

### 3.4 Catalog Service

**Bounded Context:** Product Search and Discovery

**Responsibilities:**
- Full-text product search with relevance ranking
- Faceted search and filtering
- Product recommendations and related items
- Search analytics and query optimization
- Autocomplete and search suggestions

**Data Store:** Elasticsearch 8.x (search index) + Redis (autocomplete cache)

**Elasticsearch Index Mapping:**
```json
{
  "mappings": {
    "properties": {
      "product_id": { "type": "keyword" },
      "sku": { "type": "keyword" },
      "name": {
        "type": "text",
        "analyzer": "custom_analyzer",
        "fields": {
          "keyword": { "type": "keyword" },
          "suggest": { "type": "completion" }
        }
      },
      "description": {
        "type": "text",
        "analyzer": "custom_analyzer"
      },
      "brand": {
        "type": "keyword",
        "fields": { "text": { "type": "text" } }
      },
      "categories": {
        "type": "keyword"
      },
      "category_path": {
        "type": "text",
        "analyzer": "path_analyzer"
      },
      "price": {
        "type": "scaled_float",
        "scaling_factor": 100
      },
      "compare_at_price": {
        "type": "scaled_float",
        "scaling_factor": 100
      },
      "attributes": {
        "type": "nested",
        "properties": {
          "name": { "type": "keyword" },
          "value": { "type": "keyword" }
        }
      },
      "tags": { "type": "keyword" },
      "rating_average": { "type": "float" },
      "rating_count": { "type": "integer" },
      "in_stock": { "type": "boolean" },
      "created_at": { "type": "date" },
      "popularity_score": { "type": "float" },
      "boost_score": { "type": "float" }
    }
  },
  "settings": {
    "number_of_shards": 3,
    "number_of_replicas": 2,
    "analysis": {
      "analyzer": {
        "custom_analyzer": {
          "type": "custom",
          "tokenizer": "standard",
          "filter": ["lowercase", "asciifolding", "synonym_filter", "stemmer"]
        },
        "path_analyzer": {
          "type": "custom",
          "tokenizer": "path_hierarchy"
        }
      },
      "filter": {
        "synonym_filter": {
          "type": "synonym",
          "synonyms_path": "synonyms.txt"
        }
      }
    }
  }
}
```

**Search Query Example:**
```json
{
  "query": {
    "bool": {
      "must": [
        {
          "multi_match": {
            "query": "wireless noise cancelling headphones",
            "fields": ["name^3", "description", "brand^2", "tags"],
            "type": "best_fields",
            "fuzziness": "AUTO"
          }
        }
      ],
      "filter": [
        { "term": { "in_stock": true } },
        { "range": { "price": { "gte": 50, "lte": 300 } } },
        { "terms": { "brand": ["Sony", "Bose", "Apple"] } }
      ],
      "should": [
        { "term": { "categories": { "value": "electronics", "boost": 2 } } },
        { "range": { "rating_average": { "gte": 4.0, "boost": 1.5 } } },
        { "function_score": { "field_value_factor": { "field": "popularity_score" } } }
      ]
    }
  },
  "aggs": {
    "brands": { "terms": { "field": "brand", "size": 20 } },
    "price_ranges": {
      "range": {
        "field": "price",
        "ranges": [
          { "to": 50 },
          { "from": 50, "to": 100 },
          { "from": 100, "to": 200 },
          { "from": 200 }
        ]
      }
    },
    "ratings": { "terms": { "field": "rating_average" } },
    "attributes": {
      "nested": { "path": "attributes" },
      "aggs": {
        "attr_names": {
          "terms": { "field": "attributes.name" },
          "aggs": {
            "attr_values": { "terms": { "field": "attributes.value" } }
          }
        }
      }
    }
  },
  "highlight": {
    "fields": {
      "name": {},
      "description": { "fragment_size": 150, "number_of_fragments": 3 }
    }
  },
  "from": 0,
  "size": 24,
  "sort": [
    { "_score": "desc" },
    { "popularity_score": "desc" }
  ]
}
```

### 3.5 Order Service

**Bounded Context:** Order Lifecycle Management

**Responsibilities:**
- Shopping cart management
- Order creation and validation
- Order state machine (placed, confirmed, processing, shipped, delivered, cancelled, refunded)
- Order history and tracking
- Returns and exchanges
- Tax calculation integration
- Discount and promotion application

**Order State Machine:**
```
                    ┌─────────┐
                    │ Created │
                    └────┬────┘
                         │ validate
                    ┌────▼────┐
              ┌─────│ Pending │─────┐
              │     └────┬────┘     │
              │          │ payment  │ cancel
              │     ┌────▼────┐     │
              │     │Confirmed│     │
              │     └────┬────┘     │
              │          │ fulfill  │
              │     ┌────▼────┐     │
              │     │Processing│    │
              │     └────┬────┘     │
              │          │ ship     │
              │     ┌────▼────┐     │
              │     │ Shipped │     │
              │     └────┬────┘     │
              │          │ deliver  │
              │     ┌────▼────┐     │
              │     │Delivered├─────┤
              │     └─────────┘     │
              │                     │ return
         ┌────▼────┐          ┌────▼────┐
         │Cancelled│          │Returned │
         └─────────┘          └────┬────┘
                                   │ refund
                              ┌────▼────┐
                              │Refunded │
                              └─────────┘
```

**Schema (Key Tables):**
```sql
CREATE TABLE orders (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    order_number VARCHAR(20) UNIQUE NOT NULL,
    user_id UUID NOT NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'created',
    currency VARCHAR(3) NOT NULL DEFAULT 'USD',
    subtotal_cents BIGINT NOT NULL,
    discount_cents BIGINT DEFAULT 0,
    tax_cents BIGINT NOT NULL,
    shipping_cents BIGINT NOT NULL,
    total_cents BIGINT NOT NULL,
    shipping_address JSONB NOT NULL,
    billing_address JSONB NOT NULL,
    payment_method_id UUID,
    promotion_codes TEXT[],
    notes TEXT,
    metadata JSONB DEFAULT '{}',
    ip_address INET,
    user_agent TEXT,
    placed_at TIMESTAMPTZ,
    confirmed_at TIMESTAMPTZ,
    shipped_at TIMESTAMPTZ,
    delivered_at TIMESTAMPTZ,
    cancelled_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE order_items (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    order_id UUID NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
    product_id UUID NOT NULL,
    variant_id UUID NOT NULL,
    sku VARCHAR(50) NOT NULL,
    name VARCHAR(255) NOT NULL,
    quantity INTEGER NOT NULL CHECK (quantity > 0),
    unit_price_cents BIGINT NOT NULL,
    discount_cents BIGINT DEFAULT 0,
    tax_cents BIGINT DEFAULT 0,
    total_cents BIGINT NOT NULL,
    metadata JSONB DEFAULT '{}',
    created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE order_status_history (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    order_id UUID NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
    from_status VARCHAR(20),
    to_status VARCHAR(20) NOT NULL,
    changed_by UUID,
    reason TEXT,
    metadata JSONB DEFAULT '{}',
    created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX idx_orders_user_id ON orders(user_id);
CREATE INDEX idx_orders_status ON orders(status);
CREATE INDEX idx_orders_created_at ON orders(created_at);
CREATE INDEX idx_orders_order_number ON orders(order_number);
CREATE INDEX idx_order_items_order_id ON order_items(order_id);
CREATE INDEX idx_order_items_product_id ON order_items(product_id);
```

**Events Published:**
- `order.created` — Order placed by customer
- `order.confirmed` — Payment verified, order confirmed
- `order.processing` — Order being prepared for shipment
- `order.shipped` — Order handed to shipping carrier
- `order.delivered` — Order delivered to customer
- `order.cancelled` — Order cancelled
- `order.refunded` — Refund processed for order

### 3.6 Inventory Service

**Bounded Context:** Stock Management and Availability

**Responsibilities:**
- Real-time inventory tracking across warehouses
- Stock reservation (soft locks during checkout)
- Inventory allocation and deallocation
- Low stock alerts and reorder triggers
- Multi-warehouse inventory distribution
- Inventory reconciliation and audit

**Data Store:** PostgreSQL (inventory records) + Redis (real-time availability cache)

**Inventory Operations:**
```rust
// Reserve inventory during checkout (with distributed lock)
async fn reserve_inventory(
    reservation: InventoryReservation,
) -> Result<ReservationConfirmation, InventoryError> {
    // 1. Acquire distributed lock on SKU
    let lock = redis.lock(format!("inv:lock:{}", reservation.sku), 30).await?;

    // 2. Check available quantity
    let available = db.query_one(
        "SELECT available_quantity FROM inventory
         WHERE sku = $1 AND warehouse_id = $2
         FOR UPDATE",
        &[&reservation.sku, &reservation.warehouse_id],
    ).await?;

    if available.quantity < reservation.quantity {
        return Err(InventoryError::InsufficientStock {
            requested: reservation.quantity,
            available: available.quantity,
        });
    }

    // 3. Create reservation (soft lock)
    db.execute(
        "INSERT INTO inventory_reservations
         (id, sku, warehouse_id, quantity, order_id, expires_at)
         VALUES ($1, $2, $3, $4, $5, NOW() + INTERVAL '15 minutes')",
        &[&uuid::Uuid::new_v4(), &reservation.sku,
          &reservation.warehouse_id, &reservation.quantity,
          &reservation.order_id],
    ).await?;

    // 4. Update available quantity
    db.execute(
        "UPDATE inventory SET reserved_quantity = reserved_quantity + $1
         WHERE sku = $2 AND warehouse_id = $3",
        &[&reservation.quantity, &reservation.sku, &reservation.warehouse_id],
    ).await?;

    // 5. Update Redis cache
    redis.decrby(
        format!("inv:avail:{}:{}", reservation.sku, reservation.warehouse_id),
        reservation.quantity,
    ).await?;

    // 6. Release lock
    lock.release().await?;

    Ok(ReservationConfirmation {
        reservation_id: uuid::Uuid::new_v4(),
        expires_at: Utc::now() + Duration::minutes(15),
    })
}
```

### 3.7 Payment Service

**Bounded Context:** Payment Processing and Financial Transactions

**Responsibilities:**
- Payment method management (credit cards, digital wallets, bank transfers)
- Payment processing via multiple payment gateway integrations
- Refund processing
- Payment reconciliation
- PCI DSS compliance
- Fraud detection integration

**Supported Payment Gateways:**

| Gateway | Regions | Payment Methods |
|---------|---------|-----------------|
| Stripe | Global | Cards, Apple Pay, Google Pay, bank transfers |
| Adyen | Global | Cards, iDEAL, Klarna, Alipay, WeChat Pay |
| PayPal | Global | PayPal, Venmo, Pay Later |
| Square | US, CA, UK, AU, JP | Cards, Cash App Pay |

**Payment Processing Flow:**
```
Customer → Checkout → Create Payment Intent → Gateway Authorization
                                                      │
                                               ┌──────▼──────┐
                                               │  Authorized  │
                                               └──────┬──────┘
                                                      │
                                               Order Confirmed
                                                      │
                                               ┌──────▼──────┐
                                               │  Captured    │
                                               └──────┬──────┘
                                                      │
                                          ┌───────────┼───────────┐
                                          │                       │
                                   ┌──────▼──────┐        ┌──────▼──────┐
                                   │  Settled     │        │  Refunded   │
                                   └─────────────┘        └─────────────┘
```

**PCI DSS Compliance:**
The Payment Service is designed to minimize PCI scope through tokenization. Sensitive card data never touches NexusCommerce servers. Instead, card details are tokenized client-side using the payment gateway's JavaScript SDK, and only tokens are transmitted to the backend.

```
Browser → Gateway JS SDK → Payment Token → NexusCommerce API → Gateway API
                                    (never raw card data)
```

### 3.8 Shipping Service

**Bounded Context:** Shipping and Logistics

**Responsibilities:**
- Shipping rate calculation from multiple carriers
- Shipping label generation
- Shipment tracking and status updates
- Delivery estimation
- Returns label generation
- Carrier performance analytics

**Carrier Integrations:**
- FedEx, UPS, USPS, DHL Express, DHL eCommerce
- Canada Post, Royal Mail, Australia Post
- Regional carriers via EasyPost aggregation

**Shipping Rate Calculation:**
```rust
#[derive(Debug, Serialize)]
pub struct ShippingRateRequest {
    pub origin: Address,
    pub destination: Address,
    pub packages: Vec<Package>,
    pub service_levels: Vec<ServiceLevel>, // express, standard, economy
    pub insurance_required: bool,
    pub signature_required: bool,
}

#[derive(Debug, Serialize)]
pub struct Package {
    pub weight_grams: u32,
    pub length_cm: f32,
    pub width_cm: f32,
    pub height_cm: f32,
    pub value_cents: u64,
    pub description: String,
    pub hs_code: Option<String>, // Harmonized System code for international
}

#[derive(Debug, Deserialize)]
pub struct ShippingRate {
    pub carrier: String,
    pub service: String,
    pub rate_cents: u64,
    pub currency: String,
    pub estimated_days: RangeInclusive<u32>,
    pub tracking_available: bool,
    pub insurance_included: bool,
    pub customs_duties_included: bool, // DDP vs DDU
}
```

### 3.9 Notification Service

**Bounded Context:** Customer and Internal Communications

**Responsibilities:**
- Transactional email delivery (order confirmation, shipping updates, password reset)
- SMS notifications
- Push notifications (mobile and web)
- In-app notifications
- Notification template management
- Delivery tracking and analytics
- Preference management and opt-out handling

**Notification Channels:**

| Channel | Provider | Use Cases |
|---------|----------|-----------|
| Email | AWS SES | Order confirmations, receipts, marketing |
| SMS | Twilio | OTP codes, delivery alerts |
| Push (Mobile) | Firebase Cloud Messaging | Order updates, promotions |
| Push (Web) | Web Push API | Cart abandonment, back-in-stock |
| In-App | Custom (WebSocket) | Real-time notifications |
| Slack | Slack API | Internal alerts, operational notifications |

**Template Engine:**
```handlebars
<!-- Order Confirmation Email Template -->
Subject: Order #{{order.order_number}} Confirmed!

<html>
<body>
  <h1>Thank you for your order, {{user.first_name}}!</h1>

  <p>Your order #{{order.order_number}} has been confirmed
     and is being prepared for shipment.</p>

  <table>
    <thead>
      <tr><th>Item</th><th>Qty</th><th>Price</th></tr>
    </thead>
    <tbody>
      {{#each order.items}}
      <tr>
        <td>{{this.name}}</td>
        <td>{{this.quantity}}</td>
        <td>{{format_currency this.total_cents order.currency}}</td>
      </tr>
      {{/each}}
    </tbody>
    <tfoot>
      <tr><td colspan="2">Subtotal</td><td>{{format_currency order.subtotal_cents order.currency}}</td></tr>
      <tr><td colspan="2">Shipping</td><td>{{format_currency order.shipping_cents order.currency}}</td></tr>
      <tr><td colspan="2">Tax</td><td>{{format_currency order.tax_cents order.currency}}</td></tr>
      <tr><td colspan="2"><strong>Total</strong></td><td><strong>{{format_currency order.total_cents order.currency}}</strong></td></tr>
    </tfoot>
  </table>

  <h2>Shipping Address</h2>
  <p>
    {{order.shipping_address.first_name}} {{order.shipping_address.last_name}}<br>
    {{order.shipping_address.street_line_1}}<br>
    {{#if order.shipping_address.street_line_2}}{{order.shipping_address.street_line_2}}<br>{{/if}}
    {{order.shipping_address.city}}, {{order.shipping_address.state}} {{order.shipping_address.postal_code}}<br>
    {{order.shipping_address.country_code}}
  </p>
</body>
</html>
```

### 3.10 Analytics Service

**Bounded Context:** Business Intelligence and Reporting

**Responsibilities:**
- Real-time event ingestion and processing
- Business metrics calculation (revenue, conversion, AOV, LTV)
- Pre-aggregated dashboards and reports
- Funnel analysis and cohort analysis
- A/B test result analysis
- Data export for external BI tools

**Data Store:** ClickHouse (analytics) + S3 (raw event archive)

**ClickHouse Schema:**
```sql
CREATE TABLE events (
    event_id UUID,
    event_type LowCardinality(String),
    event_timestamp DateTime64(3),
    user_id UUID,
    session_id UUID,
    device_type LowCardinality(String),
    country LowCardinality(String),
    properties String, -- JSON
    created_at DateTime DEFAULT now()
)
ENGINE = MergeTree()
PARTITION BY toYYYYMM(event_timestamp)
ORDER BY (event_type, event_timestamp, user_id)
TTL event_timestamp + INTERVAL 2 YEAR
SETTINGS index_granularity = 8192;

CREATE MATERIALIZED VIEW daily_revenue_mv
ENGINE = SummingMergeTree()
PARTITION BY toYYYYMM(date)
ORDER BY (date, country, product_category)
AS SELECT
    toDate(event_timestamp) as date,
    JSONExtractString(properties, 'country') as country,
    JSONExtractString(properties, 'product_category') as product_category,
    count() as order_count,
    sum(JSONExtractFloat(properties, 'total_cents')) as revenue_cents,
    uniq(user_id) as unique_customers
FROM events
WHERE event_type = 'order.confirmed'
GROUP BY date, country, product_category;
```

---

## 4. Database Design Patterns

### 4.1 Database per Service

Each microservice owns its database instance. No service accesses another service's database directly. This ensures loose coupling, independent schema evolution, and technology flexibility.

| Service | Database | Version | Instance Type | Storage |
|---------|----------|---------|---------------|---------|
| User | PostgreSQL | 16.2 | db.r6g.xlarge | 500 GB |
| Auth | Redis Cluster | 7.2 | cache.r6g.xlarge | 100 GB |
| Product | PostgreSQL | 16.2 | db.r6g.2xlarge | 1 TB |
| Catalog | Elasticsearch | 8.12 | r6g.2xlarge.search | 2 TB |
| Order | PostgreSQL | 16.2 | db.r6g.4xlarge | 2 TB |
| Inventory | PostgreSQL + Redis | 16.2 / 7.2 | db.r6g.xlarge + cache.r6g.large | 200 GB |
| Payment | PostgreSQL | 16.2 | db.r6g.2xlarge | 500 GB |
| Shipping | PostgreSQL | 16.2 | db.r6g.xlarge | 300 GB |
| Notification | PostgreSQL | 16.2 | db.r6g.large | 200 GB |
| Analytics | ClickHouse | 24.1 | r6g.4xlarge | 10 TB |

### 4.2 Migration Strategy

Database migrations are managed using `sqlx` migrate with version-controlled migration files:

```
migrations/
├── 20260101000000_create_users_table.sql
├── 20260101000001_create_addresses_table.sql
├── 20260115000000_add_user_preferences.sql
├── 20260201000000_add_user_metadata_column.sql
└── 20260301000000_add_user_segments.sql
```

Migration rules:
1. Migrations must be backward-compatible (no dropping columns that are still read by the previous version)
2. Column additions must have default values or be nullable
3. Index creation must use `CONCURRENTLY` to avoid table locks
4. Large data migrations must be performed in batches with progress tracking
5. Every migration must have a corresponding rollback script

### 4.3 Connection Pooling

All services use PgBouncer for PostgreSQL connection pooling:

```ini
; pgbouncer.ini
[databases]
users_db = host=users-db.internal port=5432 dbname=users

[pgbouncer]
listen_port = 6432
pool_mode = transaction
max_client_conn = 1000
default_pool_size = 50
min_pool_size = 10
reserve_pool_size = 10
reserve_pool_timeout = 3
max_db_connections = 100
server_idle_timeout = 300
client_idle_timeout = 600
query_timeout = 30
```

### 4.4 Read Replicas and CQRS

High-traffic read operations are directed to read replicas to reduce load on primary databases:

```
Write Path:  Application → Primary Database
Read Path:   Application → Read Replica (async replication, < 100ms lag)
```

For the Catalog Service, a full CQRS pattern is implemented:
- **Write Side:** Product Service writes to PostgreSQL
- **Event Bridge:** Product change events published to Kafka
- **Read Side:** Catalog Service consumes events, updates Elasticsearch index
- **Query Path:** Client queries go directly to Elasticsearch for sub-100ms search responses

---

## 5. API Gateway and Authentication

### 5.1 Kong API Gateway Configuration

The API Gateway (Kong) serves as the single entry point for all external API traffic. It handles routing, rate limiting, authentication, request transformation, and logging.

**Kong Configuration (Declarative):**
```yaml
_format_version: "3.0"

services:
  - name: user-service
    url: http://user-service.default.svc.cluster.local:8080
    routes:
      - name: user-routes
        paths:
          - /api/v1/users
        methods: [GET, POST, PUT, DELETE]
        strip_path: false

  - name: product-service
    url: http://product-service.default.svc.cluster.local:8080
    routes:
      - name: product-routes
        paths:
          - /api/v1/products
          - /api/v1/categories
        methods: [GET, POST, PUT, DELETE]
        strip_path: false

  - name: order-service
    url: http://order-service.default.svc.cluster.local:8080
    routes:
      - name: order-routes
        paths:
          - /api/v1/orders
          - /api/v1/cart
        methods: [GET, POST, PUT, DELETE]
        strip_path: false

plugins:
  - name: rate-limiting
    config:
      minute: 60
      hour: 1000
      policy: redis
      redis_host: redis-ratelimit.default.svc.cluster.local
      redis_port: 6379
      hide_client_headers: false

  - name: jwt
    config:
      uri_param_names: []
      claims_to_verify:
        - exp
      key_claim_name: kid
      header_names:
        - Authorization

  - name: cors
    config:
      origins:
        - https://www.nexuscommerce.com
        - https://admin.nexuscommerce.com
      methods:
        - GET
        - POST
        - PUT
        - DELETE
        - OPTIONS
      headers:
        - Authorization
        - Content-Type
        - X-Request-ID
      credentials: true
      max_age: 3600

  - name: request-transformer
    config:
      add:
        headers:
          - "X-Request-ID:$(uuid)"
          - "X-Forwarded-Proto:https"

  - name: response-transformer
    config:
      remove:
        headers:
          - Server
          - X-Powered-By

  - name: ip-restriction
    service: admin-service
    config:
      allow:
        - 10.0.0.0/8
        - 172.16.0.0/12
```

### 5.2 Authentication Flow

```
┌────────┐     ┌───────────┐     ┌──────────────┐     ┌──────────────┐
│ Client │────>│ API       │────>│ Auth Service  │────>│ User Service │
│        │     │ Gateway   │     │              │     │              │
└────────┘     └───────────┘     └──────────────┘     └──────────────┘
     │              │                    │                    │
     │  1. POST /auth/login              │                    │
     │──────────────>│                   │                    │
     │              │  2. Forward        │                    │
     │              │──────────────────>│                    │
     │              │                   │  3. Verify user    │
     │              │                   │───────────────────>│
     │              │                   │  4. User data      │
     │              │                   │<───────────────────│
     │              │  5. JWT tokens    │                    │
     │              │<──────────────────│                    │
     │  6. Access + Refresh tokens      │                    │
     │<──────────────│                   │                    │
     │              │                    │                    │
     │  7. GET /api/v1/orders (Bearer token)                 │
     │──────────────>│                   │                    │
     │              │  8. Validate JWT  │                    │
     │              │──────────────────>│                    │
     │              │  9. Token valid   │                    │
     │              │<──────────────────│                    │
     │              │  10. Forward to   │                    │
     │              │  Order Service    │                    │
     │              │─────────────────────────────────────>  │
```

### 5.3 API Versioning Strategy

NexusCommerce uses URL-based API versioning:

- Current stable version: `v1` (`/api/v1/...`)
- Deprecated version: `v0` (removed after March 2026)
- Beta features: `v2-beta` (`/api/v2-beta/...`)

**Deprecation Policy:**
1. New API version announced with 6 months notice
2. Previous version enters deprecation period (12 months)
3. During deprecation: `Sunset` and `Deprecation` headers added to all responses
4. After deprecation: endpoints return `410 Gone`

```
Deprecation: true
Sunset: Sat, 01 Mar 2027 00:00:00 GMT
Link: <https://docs.nexuscommerce.com/migration/v1-to-v2>; rel="successor-version"
```

---

## 6. Message Queue and Event-Driven Architecture

### 6.1 Kafka Cluster Configuration

The platform uses a dedicated Kafka cluster for asynchronous event streaming:

```yaml
# Kafka Cluster Specification
Cluster:
  Name: nexus-kafka-prod
  Version: 3.7.0
  Brokers: 6
  Instance Type: kafka.m5.4xlarge
  Storage: 2 TB per broker (gp3 SSD)
  Replication Factor: 3
  Min In-Sync Replicas: 2
  Retention: 7 days (default), 30 days (audit topics)

Topics:
  - name: user.events
    partitions: 12
    retention: 7d
    cleanup_policy: delete

  - name: product.events
    partitions: 24
    retention: 7d
    cleanup_policy: delete

  - name: order.events
    partitions: 48
    retention: 30d
    cleanup_policy: delete

  - name: payment.events
    partitions: 24
    retention: 90d
    cleanup_policy: delete

  - name: inventory.events
    partitions: 24
    retention: 7d
    cleanup_policy: delete

  - name: notification.commands
    partitions: 12
    retention: 3d
    cleanup_policy: delete

  - name: analytics.events
    partitions: 48
    retention: 7d
    cleanup_policy: delete

  - name: dead-letter
    partitions: 12
    retention: 30d
    cleanup_policy: compact
```

### 6.2 Event Schema

All events follow a standardized envelope format using CloudEvents specification:

```json
{
  "specversion": "1.0",
  "id": "evt_abc123def456",
  "source": "nexuscommerce/order-service",
  "type": "com.nexuscommerce.order.confirmed",
  "datacontenttype": "application/json",
  "time": "2026-03-20T14:30:00.000Z",
  "subject": "ord_xyz789",
  "data": {
    "order_id": "ord_xyz789",
    "order_number": "NC-2026-0312456",
    "user_id": "usr_abc123",
    "total_cents": 15999,
    "currency": "USD",
    "items": [
      {
        "product_id": "prd_456",
        "variant_id": "var_789",
        "sku": "WH-1000XM5-BLK",
        "quantity": 1,
        "unit_price_cents": 34999
      }
    ],
    "confirmed_at": "2026-03-20T14:30:00.000Z"
  },
  "traceparent": "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
}
```

### 6.3 Saga Pattern — Order Fulfillment

The order fulfillment process spans multiple services and uses the choreography-based saga pattern:

```
Step 1: Order Service publishes order.created
         │
Step 2: ├─> Inventory Service consumes → reserves stock
         │   └─> publishes inventory.reserved (success)
         │   └─> publishes inventory.reservation_failed (failure → compensate)
         │
Step 3: ├─> Payment Service consumes inventory.reserved → processes payment
         │   └─> publishes payment.captured (success)
         │   └─> publishes payment.failed (failure → compensate: release inventory)
         │
Step 4: ├─> Order Service consumes payment.captured → confirms order
         │   └─> publishes order.confirmed
         │
Step 5: ├─> Shipping Service consumes order.confirmed → creates shipment
         │   └─> publishes shipment.created
         │
Step 6: ├─> Notification Service consumes order.confirmed → sends confirmation email
         │
Step 7: └─> Analytics Service consumes order.confirmed → records metrics
```

**Compensation (Rollback) Scenarios:**

| Failure Point | Compensation Actions |
|---------------|---------------------|
| Inventory reservation fails | Cancel order, notify customer |
| Payment capture fails | Release inventory reservation, cancel order, notify customer |
| Shipping creation fails | Refund payment, release inventory, cancel order, notify customer |

### 6.4 Dead Letter Queue

Events that fail processing after maximum retry attempts are sent to the dead letter topic:

```json
{
  "original_event": { "...original event..." },
  "error": {
    "message": "Payment gateway timeout after 3 retries",
    "code": "GATEWAY_TIMEOUT",
    "stack_trace": "...",
    "service": "payment-service",
    "instance": "payment-service-7b4f8d-xz9k2"
  },
  "retry_history": [
    { "attempt": 1, "timestamp": "2026-03-20T14:30:05Z", "error": "Connection timeout" },
    { "attempt": 2, "timestamp": "2026-03-20T14:30:35Z", "error": "Connection timeout" },
    { "attempt": 3, "timestamp": "2026-03-20T14:31:35Z", "error": "Connection timeout" }
  ],
  "dead_lettered_at": "2026-03-20T14:31:36Z"
}
```

An automated alerting system monitors the dead letter queue and notifies the on-call team when events accumulate. A replay mechanism allows operators to reprocess dead-lettered events after the underlying issue is resolved.

---

## 7. Data Pipeline and Analytics

### 7.1 Real-Time Analytics Pipeline

```
Services → Kafka Topics → Kafka Streams/Flink → ClickHouse → Grafana Dashboards
                                    │
                                    └──> S3 (Raw Event Archive)
```

**Apache Flink Jobs:**

| Job Name | Input Topic | Processing | Output |
|----------|------------|------------|--------|
| Revenue Aggregator | order.events | Sum revenue by time window (1m, 5m, 1h) | ClickHouse: revenue_metrics |
| Conversion Funnel | analytics.events | Track user journey through funnel stages | ClickHouse: funnel_events |
| Inventory Alerts | inventory.events | Detect low stock conditions | Notification: notification.commands |
| Fraud Detector | payment.events | ML model scoring for fraud risk | Payment: fraud_scores |
| Session Builder | analytics.events | Sessionize user events (30m inactivity gap) | ClickHouse: sessions |

### 7.2 Batch Analytics Pipeline

```
S3 (Raw Events) → Apache Spark (EMR) → Parquet Files → Data Warehouse
                                              │
                                              └──> BI Tools (Looker, Tableau)
```

**Daily Batch Jobs:**
- Customer segmentation (RFM analysis)
- Product recommendation model training
- Revenue reconciliation
- Cohort analysis
- Customer lifetime value calculation

---

## 8. Deployment and CI/CD Pipeline

### 8.1 CI/CD Pipeline Architecture

```
Developer → Git Push → GitHub Actions → Build → Test → Security Scan →
    Deploy Staging → Integration Tests → Deploy Production (Canary) →
    Monitor → Full Rollout
```

### 8.2 GitHub Actions Workflow

```yaml
name: CI/CD Pipeline
on:
  push:
    branches: [main, release/*]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always
  REGISTRY: ghcr.io
  IMAGE_PREFIX: ghcr.io/nexuscommerce

jobs:
  lint-and-format:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - run: cargo fmt --all -- --check
      - run: cargo clippy --all-targets --all-features -- -D warnings

  test:
    runs-on: ubuntu-latest
    services:
      postgres:
        image: postgres:16
        env:
          POSTGRES_PASSWORD: test
        ports: ['5432:5432']
      redis:
        image: redis:7
        ports: ['6379:6379']
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --all-features --workspace
      - run: cargo test --all-features --workspace -- --ignored  # Integration tests

  security-scan:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Run cargo audit
        run: |
          cargo install cargo-audit
          cargo audit
      - name: Run Trivy vulnerability scanner
        uses: aquasecurity/trivy-action@master
        with:
          scan-type: 'fs'
          scan-ref: '.'
          severity: 'CRITICAL,HIGH'

  build-and-push:
    needs: [lint-and-format, test, security-scan]
    runs-on: ubuntu-latest
    if: github.ref == 'refs/heads/main' || startsWith(github.ref, 'refs/heads/release/')
    strategy:
      matrix:
        service:
          - user-service
          - auth-service
          - product-service
          - catalog-service
          - order-service
          - inventory-service
          - payment-service
          - shipping-service
          - notification-service
          - analytics-service
    steps:
      - uses: actions/checkout@v4
      - name: Build and push Docker image
        run: |
          docker build \
            --build-arg SERVICE=${{ matrix.service }} \
            -t ${{ env.IMAGE_PREFIX }}/${{ matrix.service }}:${{ github.sha }} \
            -t ${{ env.IMAGE_PREFIX }}/${{ matrix.service }}:latest \
            -f Dockerfile .
          docker push ${{ env.IMAGE_PREFIX }}/${{ matrix.service }}:${{ github.sha }}
          docker push ${{ env.IMAGE_PREFIX }}/${{ matrix.service }}:latest

  deploy-staging:
    needs: build-and-push
    runs-on: ubuntu-latest
    environment: staging
    steps:
      - uses: actions/checkout@v4
      - name: Update ArgoCD application
        run: |
          yq e ".spec.source.targetRevision = \"${{ github.sha }}\"" \
            -i k8s/overlays/staging/kustomization.yaml
          git add . && git commit -m "Deploy ${{ github.sha }} to staging"
          git push

  integration-tests:
    needs: deploy-staging
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Run integration tests
        run: |
          cargo test --test integration -- --test-threads=1
        env:
          API_BASE_URL: https://staging-api.nexuscommerce.com

  deploy-production:
    needs: integration-tests
    runs-on: ubuntu-latest
    environment: production
    steps:
      - uses: actions/checkout@v4
      - name: Deploy canary (10% traffic)
        run: |
          kubectl apply -f k8s/overlays/production/canary.yaml
      - name: Monitor canary (5 minutes)
        run: |
          ./scripts/monitor-canary.sh --duration=300 --error-threshold=0.01
      - name: Promote canary to full rollout
        run: |
          kubectl apply -f k8s/overlays/production/full-rollout.yaml
```

### 8.3 Kubernetes Deployment

**Deployment Manifest:**
```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: order-service
  namespace: production
  labels:
    app: order-service
    version: v1
spec:
  replicas: 6
  strategy:
    type: RollingUpdate
    rollingUpdate:
      maxSurge: 2
      maxUnavailable: 1
  selector:
    matchLabels:
      app: order-service
  template:
    metadata:
      labels:
        app: order-service
        version: v1
      annotations:
        prometheus.io/scrape: "true"
        prometheus.io/port: "9090"
        prometheus.io/path: "/metrics"
    spec:
      serviceAccountName: order-service
      containers:
        - name: order-service
          image: ghcr.io/nexuscommerce/order-service:abc123def
          ports:
            - containerPort: 8080
              name: http
            - containerPort: 50051
              name: grpc
            - containerPort: 9090
              name: metrics
          env:
            - name: DATABASE_URL
              valueFrom:
                secretKeyRef:
                  name: order-service-secrets
                  key: database-url
            - name: KAFKA_BROKERS
              value: "kafka-0.kafka:9092,kafka-1.kafka:9092,kafka-2.kafka:9092"
            - name: REDIS_URL
              valueFrom:
                secretKeyRef:
                  name: order-service-secrets
                  key: redis-url
            - name: OTEL_EXPORTER_OTLP_ENDPOINT
              value: "http://otel-collector.observability:4317"
            - name: RUST_LOG
              value: "order_service=info,tower_http=info"
          resources:
            requests:
              cpu: "500m"
              memory: "512Mi"
            limits:
              cpu: "2000m"
              memory: "2Gi"
          readinessProbe:
            httpGet:
              path: /health/ready
              port: 8080
            initialDelaySeconds: 5
            periodSeconds: 10
          livenessProbe:
            httpGet:
              path: /health/live
              port: 8080
            initialDelaySeconds: 15
            periodSeconds: 20
          lifecycle:
            preStop:
              exec:
                command: ["/bin/sh", "-c", "sleep 10"]
      topologySpreadConstraints:
        - maxSkew: 1
          topologyKey: topology.kubernetes.io/zone
          whenUnsatisfiable: DoNotSchedule
          labelSelector:
            matchLabels:
              app: order-service
```

### 8.4 Horizontal Pod Autoscaler

```yaml
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: order-service-hpa
  namespace: production
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: order-service
  minReplicas: 4
  maxReplicas: 50
  behavior:
    scaleUp:
      stabilizationWindowSeconds: 60
      policies:
        - type: Percent
          value: 100
          periodSeconds: 60
    scaleDown:
      stabilizationWindowSeconds: 300
      policies:
        - type: Percent
          value: 10
          periodSeconds: 60
  metrics:
    - type: Resource
      resource:
        name: cpu
        target:
          type: Utilization
          averageUtilization: 70
    - type: Resource
      resource:
        name: memory
        target:
          type: Utilization
          averageUtilization: 80
    - type: Pods
      pods:
        metric:
          name: http_requests_per_second
        target:
          type: AverageValue
          averageValue: "1000"
```

---

## 9. Monitoring and Observability

### 9.1 Three Pillars of Observability

**Metrics (Prometheus + Grafana):**

Key metrics collected from every service:

| Metric | Type | Description |
|--------|------|-------------|
| `http_requests_total` | Counter | Total HTTP requests by method, path, status |
| `http_request_duration_seconds` | Histogram | Request latency distribution |
| `grpc_server_handled_total` | Counter | Total gRPC calls by method and status |
| `grpc_server_handling_seconds` | Histogram | gRPC call latency |
| `db_query_duration_seconds` | Histogram | Database query latency |
| `db_connections_active` | Gauge | Active database connections |
| `kafka_consumer_lag` | Gauge | Kafka consumer group lag |
| `kafka_messages_produced_total` | Counter | Messages published to Kafka |
| `cache_hits_total` | Counter | Cache hit count |
| `cache_misses_total` | Counter | Cache miss count |
| `business_orders_total` | Counter | Total orders by status |
| `business_revenue_cents` | Counter | Total revenue in cents |

**Alerting Rules:**
```yaml
groups:
  - name: service-health
    rules:
      - alert: HighErrorRate
        expr: |
          sum(rate(http_requests_total{status=~"5.."}[5m])) by (service)
          / sum(rate(http_requests_total[5m])) by (service) > 0.01
        for: 5m
        labels:
          severity: critical
        annotations:
          summary: "High error rate on {{ $labels.service }}"
          description: "Error rate is {{ $value | humanizePercentage }} (threshold: 1%)"

      - alert: HighLatency
        expr: |
          histogram_quantile(0.99, rate(http_request_duration_seconds_bucket[5m])) > 1.0
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High p99 latency on {{ $labels.service }}"

      - alert: KafkaConsumerLag
        expr: kafka_consumer_lag > 10000
        for: 10m
        labels:
          severity: warning
        annotations:
          summary: "Kafka consumer lag on {{ $labels.consumer_group }}"

      - alert: DatabaseConnectionExhaustion
        expr: db_connections_active / db_connections_max > 0.9
        for: 5m
        labels:
          severity: critical
```

**Logs (Loki + Grafana):**

All services emit structured JSON logs:
```json
{
  "timestamp": "2026-03-20T14:30:00.123Z",
  "level": "INFO",
  "service": "order-service",
  "instance": "order-service-7b4f8d-xz9k2",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "span_id": "00f067aa0ba902b7",
  "message": "Order confirmed",
  "order_id": "ord_xyz789",
  "user_id": "usr_abc123",
  "total_cents": 15999,
  "duration_ms": 45
}
```

**Distributed Tracing (OpenTelemetry + Jaeger):**

Every request is assigned a trace ID that propagates across all services. This enables end-to-end request tracing for debugging and performance analysis.

```
Trace: 4bf92f3577b34da6a3ce929d0e0e4736

├── API Gateway (12ms)
│   └── Auth validation (3ms)
├── Order Service - POST /api/v1/orders (245ms)
│   ├── Validate order (5ms)
│   ├── Inventory Service - gRPC ReserveStock (45ms)
│   │   ├── Redis lock acquire (2ms)
│   │   ├── PostgreSQL query (8ms)
│   │   └── Redis cache update (3ms)
│   ├── Payment Service - gRPC ProcessPayment (150ms)
│   │   ├── Stripe API call (120ms)
│   │   └── PostgreSQL insert (10ms)
│   ├── PostgreSQL insert order (15ms)
│   └── Kafka publish order.confirmed (5ms)
└── Notification Service - async (350ms)
    ├── Template rendering (10ms)
    └── AWS SES send email (330ms)
```

### 9.2 SLA Dashboard

The SLA dashboard provides a real-time view of platform health against defined service level objectives:

| SLO | Target | Current (30d) | Status |
|-----|--------|---------------|--------|
| Availability | 99.95% | 99.98% | OK |
| API Latency (p50) | < 100ms | 45ms | OK |
| API Latency (p99) | < 500ms | 320ms | OK |
| Order Success Rate | > 99.5% | 99.7% | OK |
| Payment Success Rate | > 98% | 98.5% | OK |
| Search Latency (p99) | < 200ms | 150ms | OK |
| Error Budget Remaining | > 0% | 72% | OK |

---

## 10. Disaster Recovery and Business Continuity

### 10.1 Multi-Region Architecture

NexusCommerce operates in an active-passive multi-region configuration:

| Region | Role | Services | Database |
|--------|------|----------|----------|
| us-east-1 (Virginia) | Primary (Active) | All services | Primary + Read Replicas |
| us-west-2 (Oregon) | Secondary (Standby) | All services (warm standby) | Cross-region replicas |
| eu-west-1 (Ireland) | Tertiary | Read-only services | Cross-region read replicas |

### 10.2 Backup Strategy

| Data Type | Backup Method | Frequency | Retention | RTO | RPO |
|-----------|--------------|-----------|-----------|-----|-----|
| PostgreSQL | Automated snapshots | Hourly | 30 days | 15 min | 1 hour |
| PostgreSQL | WAL archiving | Continuous | 7 days | 5 min | < 1 min |
| PostgreSQL | Logical dump (pg_dump) | Daily | 90 days | 2 hours | 24 hours |
| Redis | RDB snapshots | Every 15 min | 7 days | 5 min | 15 min |
| Elasticsearch | Index snapshots | Daily | 30 days | 1 hour | 24 hours |
| Kafka | Topic mirroring (MirrorMaker 2) | Continuous | N/A | 5 min | < 1 min |
| S3 | Cross-region replication | Continuous | Indefinite | 0 | 0 |
| ClickHouse | Backup to S3 | Daily | 90 days | 2 hours | 24 hours |

### 10.3 Failover Procedures

**Automated Failover (Database):**
Amazon RDS Multi-AZ deployments provide automatic failover. When the primary instance fails, the standby instance is promoted automatically within 60-120 seconds. Application connection strings use the RDS endpoint, which automatically resolves to the active instance.

**Regional Failover (Manual with Automation):**
1. Incident commander declares regional failover
2. Run `./scripts/failover.sh --target-region us-west-2`
3. Script executes the following steps:
   - Promote cross-region database replicas to primary
   - Update DNS records (Route 53) to point to secondary region
   - Scale up Kubernetes deployments in secondary region
   - Verify health checks pass on all services
   - Redirect Kafka consumers to secondary cluster
4. Post-failover validation (5 minutes)
5. Notify stakeholders

**Estimated Failover Time:** 10-15 minutes for full regional failover

### 10.4 Chaos Engineering

NexusCommerce practices chaos engineering to validate resilience:

**Regular Chaos Experiments:**
- Kill random pods (weekly, automated via Chaos Monkey)
- Simulate AZ failure (monthly)
- Database failover drill (monthly)
- Network partition between services (monthly)
- Kafka broker failure (monthly)
- Regional failover drill (quarterly)
- Full disaster recovery exercise (annually)

---

## 11. Scaling Strategy

### 11.1 Horizontal Scaling

All services are designed for horizontal scaling. Scaling targets by service:

| Service | Min Replicas | Max Replicas | Scaling Metric | Scale Trigger |
|---------|-------------|-------------|----------------|---------------|
| User | 3 | 20 | CPU utilization | > 70% |
| Auth | 4 | 30 | Requests/sec | > 5000 rps |
| Product | 3 | 15 | CPU utilization | > 70% |
| Catalog | 4 | 30 | Response time p99 | > 150ms |
| Order | 6 | 50 | Requests/sec | > 2000 rps |
| Inventory | 4 | 25 | CPU utilization | > 60% |
| Payment | 4 | 25 | Requests/sec | > 1000 rps |
| Shipping | 3 | 15 | CPU utilization | > 70% |
| Notification | 3 | 20 | Queue depth | > 5000 messages |
| Analytics | 3 | 15 | CPU utilization | > 70% |

### 11.2 Database Scaling

**Vertical Scaling:** Instance size upgrades for immediate capacity (applied during maintenance windows).

**Read Replicas:** Each PostgreSQL primary has 2-3 read replicas for read-heavy workloads.

**Sharding Strategy (Order Service):** At projected volumes of 5M+ orders/day, the Order database will be sharded by `user_id` hash:
- 16 shards initially, expandable to 256
- Consistent hashing for shard assignment
- Cross-shard queries routed through a query coordinator

### 11.3 Caching Strategy

```
Request → CDN Cache (static assets, 24h TTL)
    → API Gateway Cache (GET requests, 60s TTL)
        → Application Cache (Redis, per-service)
            → Database Query Cache (PgBouncer, connection-level)
                → Database (PostgreSQL)
```

| Cache Layer | Technology | Hit Rate Target | TTL |
|-------------|-----------|-----------------|-----|
| CDN | CloudFront | > 95% (static) | 24 hours |
| API Gateway | Kong | > 60% (GET) | 60 seconds |
| Product Catalog | Redis | > 90% | 5 minutes |
| User Sessions | Redis | > 99% | 1 hour |
| Inventory Availability | Redis | > 85% | 30 seconds |
| Search Results | Elasticsearch | > 70% | 2 minutes |

---

## 12. Security Architecture

### 12.1 Network Security

```
Internet → WAF (CloudFlare) → ALB → VPC
                                      │
                            ┌─────────▼──────────┐
                            │   Public Subnet     │
                            │   (API Gateway,     │
                            │    Load Balancers)   │
                            └─────────┬──────────┘
                                      │
                            ┌─────────▼──────────┐
                            │   Private Subnet    │
                            │   (App Services,    │
                            │    Kubernetes Nodes) │
                            └─────────┬──────────┘
                                      │
                            ┌─────────▼──────────┐
                            │   Data Subnet       │
                            │   (Databases,       │
                            │    Redis, Kafka)     │
                            └────────────────────┘
```

**Security Groups:**

| Security Group | Inbound Rules | Outbound Rules |
|---------------|---------------|----------------|
| ALB-SG | 443 from 0.0.0.0/0 | All to App-SG |
| App-SG | 8080 from ALB-SG, All from App-SG | All to Data-SG, 443 to 0.0.0.0/0 |
| Data-SG | 5432/6379/9092 from App-SG | None |

### 12.2 Secrets Management

All secrets are managed through AWS Secrets Manager with automatic rotation:

| Secret Type | Rotation Period | Access Control |
|-------------|----------------|----------------|
| Database passwords | 30 days | Service-specific IAM roles |
| API keys | 90 days | Least-privilege policies |
| JWT signing keys | 90 days | Auth service only |
| Encryption keys | Annual | KMS-managed, service-specific |
| TLS certificates | Auto-renewed (Let's Encrypt) | ALB and Istio |

**Kubernetes Secrets Integration:**
```yaml
apiVersion: external-secrets.io/v1beta1
kind: ExternalSecret
metadata:
  name: order-service-secrets
spec:
  refreshInterval: 1h
  secretStoreRef:
    name: aws-secrets-manager
    kind: ClusterSecretStore
  target:
    name: order-service-secrets
    creationPolicy: Owner
  data:
    - secretKey: database-url
      remoteRef:
        key: prod/order-service/database
        property: url
    - secretKey: redis-url
      remoteRef:
        key: prod/order-service/redis
        property: url
```

### 12.3 Encryption

**In Transit:**
- All external traffic encrypted with TLS 1.3 (minimum TLS 1.2)
- Inter-service communication encrypted via Istio mTLS (mutual TLS)
- Kafka client-broker communication encrypted with SASL_SSL

**At Rest:**
- PostgreSQL: AES-256 encryption via AWS RDS encryption
- Redis: AES-256 encryption via ElastiCache encryption
- S3: AES-256 server-side encryption (SSE-S3 or SSE-KMS)
- Elasticsearch: AES-256 encryption via OpenSearch encryption
- EBS volumes: AES-256 encryption

**Application-Level Encryption:**
Sensitive fields (credit card tokens, SSN, etc.) are encrypted at the application level before database storage using envelope encryption with AWS KMS.

### 12.4 Compliance and Audit

**PCI DSS Compliance:**
- Cardholder data never stored in NexusCommerce systems (tokenization via Stripe/Adyen)
- Annual PCI DSS assessment by Qualified Security Assessor (QSA)
- Quarterly vulnerability scans by Approved Scanning Vendor (ASV)
- Network segmentation between payment service and other services
- File integrity monitoring on payment service nodes
- Comprehensive audit logging with tamper-evident storage

**SOC 2 Type II:**
- Annual audit covering Security, Availability, and Confidentiality trust service criteria
- Continuous control monitoring via compliance automation platform
- Evidence collection automated through API integrations with AWS, GitHub, and Kubernetes

**GDPR Compliance:**
- Data Processing Agreements (DPAs) with all sub-processors
- Privacy Impact Assessments (PIAs) for new features involving personal data
- Data Subject Access Request (DSAR) automation pipeline
- Right to erasure implemented across all services with cascading deletion
- Data retention policies enforced via automated jobs
- Privacy by design embedded in the development process

---

## 13. Infrastructure as Code

### 13.1 Terraform Structure

```
infrastructure/
├── modules/
│   ├── vpc/
│   │   ├── main.tf
│   │   ├── variables.tf
│   │   └── outputs.tf
│   ├── eks/
│   ├── rds/
│   ├── elasticache/
│   ├── kafka/
│   ├── elasticsearch/
│   ├── s3/
│   ├── cloudfront/
│   └── monitoring/
├── environments/
│   ├── production/
│   │   ├── main.tf
│   │   ├── variables.tf
│   │   ├── terraform.tfvars
│   │   └── backend.tf
│   ├── staging/
│   └── development/
├── global/
│   ├── iam/
│   ├── route53/
│   └── ecr/
└── Makefile
```

### 13.2 Example Terraform Module

```hcl
# modules/rds/main.tf
resource "aws_db_instance" "primary" {
  identifier     = "${var.service_name}-db-primary"
  engine         = "postgres"
  engine_version = "16.2"
  instance_class = var.instance_class

  allocated_storage     = var.allocated_storage
  max_allocated_storage = var.max_allocated_storage
  storage_type          = "gp3"
  storage_encrypted     = true
  kms_key_id            = var.kms_key_arn

  db_name  = var.database_name
  username = var.master_username
  password = random_password.master.result

  multi_az               = true
  db_subnet_group_name   = aws_db_subnet_group.this.name
  vpc_security_group_ids = [var.security_group_id]

  backup_retention_period = 30
  backup_window          = "03:00-04:00"
  maintenance_window     = "Mon:04:00-Mon:05:00"

  deletion_protection = true
  skip_final_snapshot = false
  final_snapshot_identifier = "${var.service_name}-db-final-snapshot"

  performance_insights_enabled    = true
  monitoring_interval             = 30
  enabled_cloudwatch_logs_exports = ["postgresql", "upgrade"]

  parameter_group_name = aws_db_parameter_group.this.name

  tags = merge(var.tags, {
    Service     = var.service_name
    Environment = var.environment
    ManagedBy   = "terraform"
  })
}

resource "aws_db_instance" "replica" {
  count = var.read_replica_count

  identifier          = "${var.service_name}-db-replica-${count.index + 1}"
  replicate_source_db = aws_db_instance.primary.identifier
  instance_class      = var.replica_instance_class

  storage_encrypted = true
  kms_key_id        = var.kms_key_arn

  vpc_security_group_ids = [var.security_group_id]

  performance_insights_enabled = true
  monitoring_interval          = 30

  tags = merge(var.tags, {
    Service     = var.service_name
    Environment = var.environment
    Role        = "read-replica"
    ManagedBy   = "terraform"
  })
}
```

---

## Appendix A: Service Port Assignments

| Service | HTTP Port | gRPC Port | Metrics Port |
|---------|-----------|-----------|-------------|
| User Service | 8080 | 50051 | 9090 |
| Auth Service | 8081 | 50052 | 9091 |
| Product Service | 8082 | 50053 | 9092 |
| Catalog Service | 8083 | 50054 | 9093 |
| Order Service | 8084 | 50055 | 9094 |
| Inventory Service | 8085 | 50056 | 9095 |
| Payment Service | 8086 | 50057 | 9096 |
| Shipping Service | 8087 | 50058 | 9097 |
| Notification Service | 8088 | 50059 | 9098 |
| Analytics Service | 8089 | 50060 | 9099 |

## Appendix B: Environment Variables

All services share a common set of environment variables:

| Variable | Description | Example |
|----------|-------------|---------|
| `SERVICE_NAME` | Service identifier | `order-service` |
| `SERVICE_PORT` | HTTP listen port | `8080` |
| `GRPC_PORT` | gRPC listen port | `50055` |
| `METRICS_PORT` | Prometheus metrics port | `9094` |
| `DATABASE_URL` | PostgreSQL connection string | `postgres://...` |
| `REDIS_URL` | Redis connection string | `redis://...` |
| `KAFKA_BROKERS` | Kafka bootstrap servers | `kafka-0:9092,...` |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | OpenTelemetry collector | `http://otel:4317` |
| `RUST_LOG` | Log level configuration | `info` |
| `JWT_PUBLIC_KEY` | JWT verification key | PEM-encoded RSA public key |
| `ENVIRONMENT` | Deployment environment | `production` |
| `AWS_REGION` | AWS region | `us-east-1` |

## Appendix C: Runbook Quick Reference

| Scenario | Runbook | Escalation |
|----------|---------|------------|
| Service unresponsive | Restart pod; check logs; verify dependencies | On-call SRE |
| High error rate (> 1%) | Check logs; identify error pattern; rollback if recent deploy | Service owner + SRE |
| Database connection exhaustion | Scale up connections; identify connection leaks | DBA + SRE |
| Kafka consumer lag > 10k | Scale consumers; check for processing errors | Service owner |
| Payment gateway timeout | Check gateway status page; switch to backup gateway | Payment team lead |
| Memory leak detected | Heap dump analysis; rollback to previous version | Service owner |
| DDoS attack detected | Verify WAF rules; enable rate limiting; engage CloudFlare | Security team |
| Data breach suspected | Isolate affected systems; invoke incident response plan | CISO + Legal |
| Regional outage | Execute regional failover procedure | Incident commander |
| Certificate expiration | Renew via ACM or Let's Encrypt; verify auto-renewal | SRE |

## Appendix D: Decision Records

| ADR | Title | Date | Status |
|-----|-------|------|--------|
| ADR-001 | Use Rust for all backend services | 2024-01 | Accepted |
| ADR-002 | Choose Kafka over RabbitMQ for event streaming | 2024-02 | Accepted |
| ADR-003 | Adopt microservices over monolith | 2024-01 | Accepted |
| ADR-004 | Use PostgreSQL as primary database | 2024-01 | Accepted |
| ADR-005 | Choose ClickHouse for analytics over BigQuery | 2024-03 | Accepted |
| ADR-006 | Use Kubernetes (EKS) for orchestration | 2024-02 | Accepted |
| ADR-007 | Adopt Istio for service mesh | 2024-04 | Accepted |
| ADR-008 | Choose Elasticsearch over Meilisearch for product search | 2024-03 | Accepted |
| ADR-009 | Implement choreography-based sagas over orchestration | 2024-05 | Accepted |
| ADR-010 | Use ArgoCD for GitOps deployments | 2024-06 | Accepted |
| ADR-011 | Adopt OpenTelemetry for observability | 2024-04 | Accepted |
| ADR-012 | Choose Kong over AWS API Gateway | 2024-03 | Accepted |
| ADR-013 | Implement active-passive multi-region over active-active | 2025-01 | Accepted |
| ADR-014 | Use Redis Cluster over Redis Sentinel | 2025-03 | Accepted |
| ADR-015 | Migrate from REST to gRPC for inter-service communication | 2025-06 | Accepted |

---

*This document is maintained by the Platform Engineering team. For questions or proposed changes, submit a pull request to the `platform-docs` repository or contact platform-eng@nexuscommerce.com.*

*Last reviewed: March 2026 | Next scheduled review: September 2026*
