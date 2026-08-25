*** Settings ***
Documentation       Provision routing by id/idPattern — redirect, exclusive
...                 and inclusive modes (Antares extension IOP TPs, ADR-001
...                 URN vocabulary). 4.3.6.3: a redirect registration keeps
...                 the registered data external ("The Context Broker
...                 itself holds no data locally in conflict to the
...                 registration"); an exclusive registration "shall define
...                 both: An entity id (i.e. an id pattern or Entity type
...                 defining a group of entities is not supported for
...                 exclusive registrations) [and] Attributes." 4.3.6.1:
...                 forwarded requests respect ALL registration
...                 constraints, including Entity IDs. 5.6.1.4 create,
...                 5.6.2.4/5.6.3.4 attribute updates, 5.6.6.4 delete,
...                 5.6.7.4/5.6.8.4/5.6.8.5 batch create/upsert
...                 distribution and success arrays.

Resource            ${EXECDIR}/resources/ApiUtils/InteropUtils.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Library             Collections
Library             RequestsLibrary
Library             HttpCtrl.Server

Test Setup          Setup Interop Ids
Test Teardown       Cleanup Interop Fixtures


*** Variables ***
${b1_url}
${b2_url}
${b3_url}
${mock_host}        127.0.0.1
${mock_port}        8089


*** Test Cases ***
IOP_EXT_IDR_03_20 A Redirect Pattern Routes Create To The Registered Source
    [Documentation]    5.6.1.4 + 4.3.6.3: POST /entities via B1 with an id
    ...    matching the redirect CSR's anchored pattern is created at B2 —
    ...    and NOT held at B1 (local=true retrieve stays 404: "The Context
    ...    Broker itself holds no data locally in conflict to the
    ...    registration").
    [Tags]    iop    iop-ext    5_12    5_6_1    4_3_6    since_v1.9.1
    Register Redirect Pattern CSR    ${pat_bb}    ${b2_url}
    ${e}=    Waste Entity    ${eid_bb}    ${0.42}
    ${response}=    Create Entity At Broker    ${b1_url}    ${e}

    ${response}=    Get Entity Via Broker    ${b2_url}    ${eid_bb}    local=true
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.json()['id']}    ${eid_bb}
    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}    local=true
    Check Response Status Code    404    ${response.status_code}
    Should Not Contain    ${response.text}    fillLevel

IOP_EXT_IDR_03_21 A Non-Matching Create Stays Local And Never Dials The Source
    [Documentation]    5.6.1.4 + 5.12: the created id does not match the
    ...    redirect pattern — the entity is created locally at B1 and the
    ...    registered source records ZERO requests.
    [Tags]    iop    iop-ext    5_12    5_6_1    since_v1.9.1
    Start Mock
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "mode": "redirect", "operations": ["redirectionOps"], "information": $info, "endpoint": "http://" + $mock_host + ":" + str($mock_port)}
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${response.status_code}

    ${e}=    Waste Entity    ${eid_presov}    ${0.7}
    Create Entity At Broker    ${b1_url}    ${e}
    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_presov}    local=true
    Check Response Status Code    200    ${response.status_code}
    Wait For No Request    ${2}

IOP_EXT_IDR_03_22 Delete Routes By Pattern And Never Leaves B1 For Foreign Ids
    [Documentation]    5.6.6.4: DELETE via B1 of a pattern-matching id is
    ...    forwarded — 204 and the entity is gone at B2; deleting a
    ...    non-matching id that exists ONLY at B2 is 404 at B1 (not
    ...    forwarded) and B2 still holds it.
    [Tags]    iop    iop-ext    5_12    5_6_6    since_v1.9.1
    Register Redirect Pattern CSR    ${pat_bb}    ${b2_url}
    ${e1}=    Waste Entity    ${eid_bb}    ${0.42}
    Create Entity At Broker    ${b2_url}    ${e1}
    ${e2}=    Waste Entity    ${eid_presov}    ${0.7}
    Create Entity At Broker    ${b2_url}    ${e2}

    ${response}=    Delete Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    204    ${response.status_code}
    ${response}=    Get Entity Via Broker    ${b2_url}    ${eid_bb}    local=true
    Check Response Status Code    404    ${response.status_code}

    ${response}=    Delete Entity Via Broker    ${b1_url}    ${eid_presov}
    Check Response Status Code    404    ${response.status_code}
    ${response}=    Get Entity Via Broker    ${b2_url}    ${eid_presov}    local=true
    Check Response Status Code    200    ${response.status_code}

IOP_EXT_IDR_03_23 Attribute Updates Route Through The Pattern
    [Documentation]    5.6.2.4/5.6.3.4: PATCH …/attrs and POST …/attrs via
    ...    B1 route to B2 through the redirect pattern — the value changes
    ...    at B2, the appended attribute appears at B2, and B1 still holds
    ...    nothing locally.
    [Tags]    iop    iop-ext    5_12    5_6_2    5_6_3    since_v1.9.1
    Register Redirect Pattern CSR    ${pat_bb}    ${b2_url}
    ${e}=    Waste Entity    ${eid_bb}    ${0.42}
    Create Entity At Broker    ${b2_url}    ${e}

    ${patch}=    Evaluate    {"fillLevel": {"type": "Property", "value": 0.99}}
    ${response}=    Patch Entity Attrs Via Broker    ${b1_url}    ${eid_bb}    ${patch}
    Check Response Status Code    204    ${response.status_code}
    ${append}=    Evaluate    {"status": {"type": "Property", "value": "servisovany"}}
    ${response}=    Append Entity Attrs Via Broker    ${b1_url}    ${eid_bb}    ${append}
    Check Response Status Code    204    ${response.status_code}

    ${response}=    Get Entity Via Broker    ${b2_url}    ${eid_bb}    local=true
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal As Numbers    ${response.json()['fillLevel']['value']}    0.99
    Should Contain    ${response.text}    servisovany
    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}    local=true
    Check Response Status Code    404    ${response.status_code}

IOP_EXT_IDR_03_24 Batch Create Splits Mixed Razidlos And Reports Every Id Once
    [Documentation]    5.6.7.4 + 5.6.8.5: a batch create with ids of two
    ...    razidlos — the pattern-matching subset is created at B2, the
    ...    rest locally at B1; the success array carries ALL ids, each
    ...    exactly once.
    [Tags]    iop    iop-ext    5_12    5_6_7    since_v1.9.1
    Register Redirect Pattern CSR    ${pat_bb}    ${b2_url}
    ${e1}=    Waste Entity    ${eid_bb}    ${0.42}
    ${e2}=    Waste Entity    ${eid_presov}    ${0.7}
    ${payload}=    Evaluate    [$e1, $e2]
    ${response}=    Batch Op Via Broker    ${b1_url}    create    ${payload}
    Should Be True    ${response.status_code} in (201, 207)
    ${n_bb}=    Evaluate    $response.text.count($eid_bb)
    Should Be Equal As Integers    ${n_bb}    1
    ${n_pr}=    Evaluate    $response.text.count($eid_presov)
    Should Be Equal As Integers    ${n_pr}    1

    ${response}=    Get Entity Via Broker    ${b2_url}    ${eid_bb}    local=true
    Check Response Status Code    200    ${response.status_code}
    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_presov}    local=true
    Check Response Status Code    200    ${response.status_code}
    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}    local=true
    Check Response Status Code    404    ${response.status_code}

IOP_EXT_IDR_03_25 A Deterministic URN Upserts Idempotently Across The Federation
    [Documentation]    5.6.8.5 (ADR-001 idempotency claim): the same upsert
    ...    twice via B1 — the first distributes a create (201 + created
    ...    list), the second is an UPDATE (204: the remote answered 204,
    ...    so the id must NOT appear in a created list) and the
    ...    federation-wide query still returns exactly ONE entity.
    [Tags]    iop    iop-ext    5_12    5_6_8    since_v1.9.1
    Register Redirect Pattern CSR    ${pat_bb}    ${b2_url}
    ${e}=    Waste Entity    ${eid_bb}    ${0.42}
    ${payload}=    Evaluate    [$e]
    ${response}=    Batch Op Via Broker    ${b1_url}    upsert    ${payload}
    Check Response Status Code    201    ${response.status_code}
    Should Contain    ${response.text}    ${eid_bb}

    ${response}=    Batch Op Via Broker    ${b1_url}    upsert    ${payload}
    Check Response Status Code    204    ${response.status_code}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    1
    ${n}=    Evaluate    $response.text.count($eid_bb)
    Should Be Equal As Integers    ${n}    1

IOP_EXT_IDR_03_26 An Exclusive CSR Requires An Exact Id And Attributes
    [Documentation]    4.3.6.3: "an id pattern or Entity type defining a
    ...    group of entities is not supported for exclusive registrations"
    ...    and the registration "shall define both" an entity id AND
    ...    Attributes — an exclusive CSR with idPattern, or with an id but
    ...    no Attributes, is 400 BadRequestData and nothing is registered.
    [Tags]    iop    iop-ext    4_3_6    5_2_9    5_9_2    since_v1.9.1
    ${endpoint}=    Broker Base Of    ${b2_url}
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}], "propertyNames": ["fillLevel"]}]
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "mode": "exclusive", "information": $info, "endpoint": $endpoint}
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    400    ${response.status_code}
    Should Contain    ${response.text}    BadRequestData

    ${info}=    Evaluate    [{"entities": [{"type": $etype, "id": $eid_bb}]}]
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "mode": "exclusive", "information": $info, "endpoint": $endpoint}
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    400    ${response.status_code}

    ${response}=    Get Registration At Broker    ${b1_url}    ${registration_id}
    Check Response Status Code    404    ${response.status_code}

IOP_EXT_IDR_03_27 An Exclusive CSR Routes Updates Without A Local Shadow
    [Documentation]    4.3.6.2/4.3.6.3: exclusive means "all of the
    ...    registered context data is held in a single location external
    ...    to the Context Broker" — the attribute update via B1 lands at
    ...    B2, the retrieve via B1 serves the remote data, and B1 never
    ...    holds a local copy (local=true 404 before AND after).
    [Tags]    iop    iop-ext    4_3_6    5_6_2    since_v1.9.1
    ${endpoint}=    Broker Base Of    ${b2_url}
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "id": $eid_bb}], "propertyNames": ["fillLevel"]}]
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "mode": "exclusive", "operations": ["redirectionOps"], "information": $info, "endpoint": $endpoint}
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${response.status_code}
    ${e}=    Waste Entity    ${eid_bb}    ${0.42}
    Create Entity At Broker    ${b2_url}    ${e}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}    local=true
    Check Response Status Code    404    ${response.status_code}
    ${patch}=    Evaluate    {"fillLevel": {"type": "Property", "value": 0.88}}
    ${response}=    Patch Entity Attrs Via Broker    ${b1_url}    ${eid_bb}    ${patch}
    Check Response Status Code    204    ${response.status_code}

    ${response}=    Get Entity Via Broker    ${b2_url}    ${eid_bb}    local=true
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal As Numbers    ${response.json()['fillLevel']['value']}    0.88
    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal As Numbers    ${response.json()['fillLevel']['value']}    0.88
    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}    local=true
    Check Response Status Code    404    ${response.status_code}

IOP_EXT_IDR_03_28 A Forwarded Batch Carries Only The Registered Id Scope
    [Documentation]    4.3.6.1 ("all constraints specified in the
    ...    registration shall be respected", incl. Entity IDs): the
    ...    forwarded provision request to the pattern-scoped source
    ...    carries ONLY the matching id — the non-matching id must NOT
    ...    appear anywhere in the request the source receives.
    [Tags]    iop    iop-ext    4_3_6    5_6_7    since_v1.9.1
    Start Mock
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "mode": "redirect", "operations": ["redirectionOps", "createBatch", "upsertBatch"], "information": $info, "endpoint": "http://" + $mock_host + ":" + str($mock_port)}
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${response.status_code}

    ${e1}=    Waste Entity    ${eid_bb}    ${0.42}
    ${e2}=    Waste Entity    ${eid_presov}    ${0.7}
    ${payload}=    Evaluate    [$e1, $e2]
    ${response}=    Batch Op Via Broker    ${b1_url}    create    ${payload}
    Should Be True    ${response.status_code} in (201, 207)
    Wait For Request
    ${body}=    Get Request Body
    ${body}=    Evaluate    $body.decode() if isinstance($body, bytes) else str($body)
    Should Contain    ${body}    ${eid_bb}
    Should Not Contain    ${body}    ${eid_presov}

IOP_EXT_IDR_03_29 A Batch Upsert Splits Three Ways Along Razidlo Prefixes
    [Documentation]    5.6.8.4: one upsert with three razidlos against two
    ...    disjoint redirect CSRs — each broker ends up holding EXACTLY
    ...    its razidlo's entity locally and none of the others (asserted
    ...    per broker with local=true).
    [Tags]    iop    iop-ext    5_12    5_6_8    since_v1.9.1
    Register Redirect Pattern CSR    ${pat_bb}    ${b2_url}
    Register Redirect Pattern CSR    ${pat_zvolen}    ${b3_url}    ${registration_id}-2
    ${e1}=    Waste Entity    ${eid_bb}    ${0.42}
    ${e2}=    Waste Entity    ${eid_zvolen}    ${0.3}
    ${e3}=    Waste Entity    ${eid_presov}    ${0.7}
    ${payload}=    Evaluate    [$e1, $e2, $e3]
    ${response}=    Batch Op Via Broker    ${b1_url}    upsert    ${payload}
    Should Be True    ${response.status_code} in (201, 204, 207)

    FOR    ${broker}    ${own}    IN
    ...    ${b2_url}    ${eid_bb}
    ...    ${b3_url}    ${eid_zvolen}
    ...    ${b1_url}    ${eid_presov}
        ${response}=    Get Entity Via Broker    ${broker}    ${own}    local=true
        Check Response Status Code    200    ${response.status_code}
    END
    FOR    ${broker}    ${foreign}    IN
    ...    ${b2_url}    ${eid_zvolen}
    ...    ${b2_url}    ${eid_presov}
    ...    ${b3_url}    ${eid_bb}
    ...    ${b1_url}    ${eid_bb}
    ...    ${b1_url}    ${eid_zvolen}
        ${response}=    Get Entity Via Broker    ${broker}    ${foreign}    local=true
        Check Response Status Code    404    ${response.status_code}
    END


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${etype}    WasteC${suffix}
    Set Test Variable    ${base}    urn:ngsi-ld:WasteC${suffix}
    ${pat_bb}=    Evaluate    "^" + $base + ":sk_banskabystrica:odpady:.*$"
    Set Test Variable    ${pat_bb}
    ${pat_zvolen}=    Evaluate    "^" + $base + ":sk_zvolen:odpady:.*$"
    Set Test Variable    ${pat_zvolen}
    Set Test Variable    ${eid_bb}    ${base}:sk_banskabystrica:odpady:kontajner:0042
    Set Test Variable    ${eid_presov}    ${base}:sk_presov:odpady:kontajner:0001
    Set Test Variable    ${eid_zvolen}    ${base}:sk_zvolen:odpady:kontajner:0007
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:iopidr-${suffix}
    Set Test Variable    ${server_started}    ${False}

Waste Entity
    [Documentation]    ADR-001-shaped WasteContainer fixture.
    [Arguments]    ${eid}    ${level}
    ${e}=    Evaluate
    ...    {"id": $eid, "type": $etype, "fillLevel": {"type": "Property", "value": $level}}
    RETURN    ${e}

Register Redirect Pattern CSR
    [Documentation]    Redirect CSR with an anchored idPattern EntityInfo;
    ...    redirectionOps + the batch ops so batch provision routes too.
    [Arguments]    ${pattern}    ${target}    ${rid}=${EMPTY}
    ${rid}=    Set Variable If    '${rid}' == ''    ${registration_id}    ${rid}
    ${endpoint}=    Broker Base Of    ${target}
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pattern}]}]
    ${reg}=    Evaluate
    ...    {"id": $rid, "type": "ContextSourceRegistration", "mode": "redirect", "operations": ["redirectionOps", "createBatch", "upsertBatch", "updateBatch", "deleteBatch"], "information": $info, "endpoint": $endpoint}
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${response.status_code}

Start Mock
    Start Server    ${mock_host}    ${mock_port}
    Set Test Variable    ${server_started}    ${True}

Cleanup Interop Fixtures
    FOR    ${rid}    IN    ${registration_id}    ${registration_id}-2
        Delete Registration At Broker    ${b1_url}    ${rid}
    END
    FOR    ${eid}    IN    ${eid_bb}    ${eid_presov}    ${eid_zvolen}
        Delete Entity Via Broker    ${b1_url}    ${eid}
        Delete Entity Via Broker    ${b2_url}    ${eid}
        Delete Entity Via Broker    ${b3_url}    ${eid}
    END
    IF    ${server_started}
        Stop Server
    END
