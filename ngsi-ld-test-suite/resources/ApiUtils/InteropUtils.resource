*** Settings ***
Documentation       Raw-request helpers for the IOP_EXT multi-broker
...                 interoperability TPs (Antares extension). Every keyword
...                 takes the target broker's API url explicitly (b1_url /
...                 b2_url / b3_url), so one test can act on several brokers
...                 and assert on each side. Plain application/json bodies,
...                 no Link header — both sides expand names against the
...                 default context, which keeps the TPs broker-agnostic.

Library         Collections
Library         RequestsLibrary
Library         String


*** Keywords ***
Broker Base Of
    [Documentation]    Registration endpoints carry the broker BASE url —
    ...    the forward appends /ngsi-ld/v1 itself.
    [Arguments]    ${api_url}
    ${base}=    Evaluate    "${api_url}".replace("/ngsi-ld/v1", "")
    RETURN    ${base}

Random Interop Suffix
    ${suffix}=    Evaluate    __import__('uuid').uuid4().hex[:8]
    RETURN    ${suffix}

Register Broker As Context Source
    [Documentation]    POST a ContextSourceRegistration at broker ${at}
    ...    whose endpoint is broker ${target} (API url; rewritten to base).
    ...    ${information} defaults to a type-level EntityInfo for ${etype}.
    [Arguments]    ${at}    ${rid}    ${target}    ${etype}
    ...    ${mode}=inclusive    ${operations}=${None}    ${information}=${None}
    ...    ${reg_tenant}=${EMPTY}    ${local_only}=${False}
    ${endpoint}=    Broker Base Of    ${target}
    ${info}=    Evaluate
    ...    $information if $information is not None else [{"entities": [{"type": $etype}]}]
    ${reg}=    Evaluate
    ...    {"id": $rid, "type": "ContextSourceRegistration", "mode": $mode, "information": $info, "endpoint": $endpoint}
    IF    $operations is not None
        Set To Dictionary    ${reg}    operations=${operations}
    END
    IF    '${reg_tenant}' != ''
        Set To Dictionary    ${reg}    tenant=${reg_tenant}
    END
    IF    ${local_only}
        Set To Dictionary    ${reg}    localOnly=${True}
    END
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST
    ...    url=${at}/csourceRegistrations
    ...    json=${reg}
    ...    headers=${headers}
    ...    expected_status=any
    Should Be Equal As Integers    ${response.status_code}    201
    RETURN    ${response}

Post Registration At Broker
    [Documentation]    5.9.2 Register Context Source — raw document, any
    ...    status (conflict TPs assert 409 themselves).
    [Arguments]    ${at}    ${reg}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST
    ...    url=${at}/csourceRegistrations
    ...    json=${reg}
    ...    headers=${headers}
    ...    expected_status=any
    RETURN    ${response}

Patch Registration At Broker
    [Documentation]    5.9.3 Update Context Source Registration.
    [Arguments]    ${at}    ${rid}    ${fragment}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${r}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($rid, safe='')
    ${response}=    PATCH
    ...    url=${at}/csourceRegistrations/${r}
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    RETURN    ${response}

Get Registration At Broker
    [Arguments]    ${at}    ${rid}
    ${r}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($rid, safe='')
    ${response}=    GET
    ...    url=${at}/csourceRegistrations/${r}
    ...    expected_status=any
    RETURN    ${response}

Create Entity At Broker
    [Arguments]    ${at}    ${entity}    ${tenant}=${EMPTY}
    &{headers}=    Create Dictionary    Content-Type=application/json
    IF    '${tenant}' != ''
        Set To Dictionary    ${headers}    NGSILD-Tenant=${tenant}
    END
    ${response}=    POST
    ...    url=${at}/entities
    ...    json=${entity}
    ...    headers=${headers}
    ...    expected_status=any
    Should Be Equal As Integers    ${response.status_code}    201
    RETURN    ${response}

Upsert Temporal At Broker
    [Documentation]    5.6.11 Create or Update Temporal Evolution.
    [Arguments]    ${at}    ${doc}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST
    ...    url=${at}/temporal/entities
    ...    json=${doc}
    ...    headers=${headers}
    ...    expected_status=any
    Should Be True    ${response.status_code} in (201, 204)
    RETURN    ${response}

Get Entity Via Broker
    [Arguments]    ${at}    ${entity_id}    ${tenant}=${EMPTY}    &{params}
    &{headers}=    Create Dictionary
    IF    '${tenant}' != ''
        Set To Dictionary    ${headers}    NGSILD-Tenant=${tenant}
    END
    ${eid}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($entity_id, safe='')
    ${response}=    GET
    ...    url=${at}/entities/${eid}
    ...    params=${params}
    ...    headers=${headers}
    ...    expected_status=any
    RETURN    ${response}

Query Entities Via Broker
    [Arguments]    ${at}    ${map_ref}=${EMPTY}    &{params}
    &{headers}=    Create Dictionary
    IF    '${map_ref}' != ''
        Set To Dictionary    ${headers}    NGSILD-EntityMap=${map_ref}
    END
    ${response}=    GET
    ...    url=${at}/entities
    ...    params=${params}
    ...    headers=${headers}
    ...    expected_status=any
    RETURN    ${response}

Get Temporal Via Broker
    [Arguments]    ${at}    ${entity_id}    &{params}
    ${eid}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($entity_id, safe='')
    ${response}=    GET
    ...    url=${at}/temporal/entities/${eid}
    ...    params=${params}
    ...    expected_status=any
    RETURN    ${response}

Query Temporal Via Broker
    [Arguments]    ${at}    &{params}
    ${response}=    GET
    ...    url=${at}/temporal/entities
    ...    params=${params}
    ...    expected_status=any
    RETURN    ${response}

Patch Entity Attrs Via Broker
    [Documentation]    5.6.2 Update Entity Attributes (PATCH .../attrs).
    [Arguments]    ${at}    ${entity_id}    ${fragment}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${eid}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($entity_id, safe='')
    ${response}=    PATCH
    ...    url=${at}/entities/${eid}/attrs
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    RETURN    ${response}

Append Entity Attrs Via Broker
    [Documentation]    5.6.3 Append Entity Attributes (POST .../attrs).
    [Arguments]    ${at}    ${entity_id}    ${fragment}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${eid}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($entity_id, safe='')
    ${response}=    POST
    ...    url=${at}/entities/${eid}/attrs
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    RETURN    ${response}

Delete Entity Attr Via Broker
    [Documentation]    5.6.5 Delete Entity Attribute.
    [Arguments]    ${at}    ${entity_id}    ${attr}
    ${eid}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($entity_id, safe='')
    ${response}=    DELETE
    ...    url=${at}/entities/${eid}/attrs/${attr}
    ...    expected_status=any
    RETURN    ${response}

Delete Entity Via Broker
    [Arguments]    ${at}    ${entity_id}    ${tenant}=${EMPTY}
    &{headers}=    Create Dictionary
    IF    '${tenant}' != ''
        Set To Dictionary    ${headers}    NGSILD-Tenant=${tenant}
    END
    ${eid}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($entity_id, safe='')
    ${response}=    DELETE
    ...    url=${at}/entities/${eid}
    ...    headers=${headers}
    ...    expected_status=any
    RETURN    ${response}

Delete Temporal Via Broker
    [Arguments]    ${at}    ${entity_id}
    ${eid}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($entity_id, safe='')
    ${response}=    DELETE
    ...    url=${at}/temporal/entities/${eid}
    ...    expected_status=any
    RETURN    ${response}

Delete Registration At Broker
    [Arguments]    ${at}    ${rid}
    ${rid}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($rid, safe='')
    ${response}=    DELETE
    ...    url=${at}/csourceRegistrations/${rid}
    ...    expected_status=any
    RETURN    ${response}

Merge Entity Via Broker
    [Documentation]    5.6.17 Merge Entity (PATCH /entities/{id}).
    [Arguments]    ${at}    ${entity_id}    ${fragment}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${eid}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($entity_id, safe='')
    ${response}=    PATCH
    ...    url=${at}/entities/${eid}
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    RETURN    ${response}

Replace Entity Via Broker
    [Documentation]    5.6.18 Replace Entity (PUT /entities/{id}).
    [Arguments]    ${at}    ${entity_id}    ${entity}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${eid}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($entity_id, safe='')
    ${response}=    PUT
    ...    url=${at}/entities/${eid}
    ...    json=${entity}
    ...    headers=${headers}
    ...    expected_status=any
    RETURN    ${response}

Replace Attr Via Broker
    [Documentation]    5.6.19 Replace Attribute (PUT /entities/{id}/attrs/{attr}).
    [Arguments]    ${at}    ${entity_id}    ${attr}    ${fragment}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${eid}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($entity_id, safe='')
    ${response}=    PUT
    ...    url=${at}/entities/${eid}/attrs/${attr}
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    RETURN    ${response}

Batch Op Via Broker
    [Documentation]    5.6.7-5.6.10 batch operations (POST /entityOperations/{op}).
    [Arguments]    ${at}    ${op}    ${payload}    &{params}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST
    ...    url=${at}/entityOperations/${op}
    ...    json=${payload}
    ...    params=${params}
    ...    headers=${headers}
    ...    expected_status=any
    RETURN    ${response}

Purge Entities Via Broker
    [Documentation]    5.6.21 Purge Entities (DELETE /entities?…).
    [Arguments]    ${at}    &{params}
    ${response}=    DELETE
    ...    url=${at}/entities
    ...    params=${params}
    ...    expected_status=any
    RETURN    ${response}

Create Subscription At Broker
    [Arguments]    ${at}    ${sub_id}    ${etype}    ${notify_uri}    ${q}=${EMPTY}
    ${sub}=    Evaluate
    ...    {"id": $sub_id, "type": "Subscription", "entities": [{"type": $etype}], "notification": {"endpoint": {"uri": $notify_uri, "accept": "application/json"}}}
    IF    '${q}' != ''
        Set To Dictionary    ${sub}    q=${q}
    END
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST
    ...    url=${at}/subscriptions
    ...    json=${sub}
    ...    headers=${headers}
    ...    expected_status=any
    Should Be Equal As Integers    ${response.status_code}    201
    RETURN    ${response}

Delete Subscription At Broker
    [Arguments]    ${at}    ${sub_id}
    ${sid}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($sub_id, safe='')
    ${response}=    DELETE
    ...    url=${at}/subscriptions/${sid}
    ...    expected_status=any
    RETURN    ${response}

Simple Vehicle Entity
    [Documentation]    Minimal entity of ${etype} with a speed Property; an
    ...    optional observedAt makes 4.5.5.3 recency deterministic.
    [Arguments]    ${entity_id}    ${etype}    ${speed}    ${observed_at}=${EMPTY}
    IF    '${observed_at}' != ''
        ${entity}=    Evaluate
        ...    {"id": $entity_id, "type": $etype, "speed": {"type": "Property", "value": int($speed), "observedAt": $observed_at}}
    ELSE
        ${entity}=    Evaluate
        ...    {"id": $entity_id, "type": $etype, "speed": {"type": "Property", "value": int($speed)}}
    END
    RETURN    ${entity}
