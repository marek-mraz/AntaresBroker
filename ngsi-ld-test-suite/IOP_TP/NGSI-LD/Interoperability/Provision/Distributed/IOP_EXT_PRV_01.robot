*** Settings ***
Documentation       Distributed provision I (Antares extension IOP TPs).
...                 5.6.1.4: create forwarded to matching redirect CSRs
...                 supporting createEntity; "For matching redirect
...                 Registrations where the Create Entity operation is not
...                 supported, this shall result in an error of type
...                 Conflict"; inclusive forwards happen only "in case the
...                 Create Entity operation is supported". 5.6.2.4: matching
...                 exclusive/redirect Attributes forwarded, "the matching
...                 Attributes are then removed from the Fragment"; a failed
...                 remote part is a partial success. 5.6.3.4 append /
...                 5.6.4.4 partial update: same forward split; unknown
...                 target Attribute → ResourceNotFound.

Resource            ${EXECDIR}/resources/ApiUtils/InteropUtils.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Library             Collections
Library             RequestsLibrary

Test Setup          Setup Interop Ids
Test Teardown       Cleanup Interop Fixtures


*** Variables ***
${b1_url}
${b2_url}
${b3_url}
${dead_endpoint}    http://127.0.0.1:8099


*** Test Cases ***
IOP_EXT_PRV_01_01 Create Redirects To The Supporting Source Only
    [Documentation]    5.6.1.4: "If any redirect Context Source
    ...    Registrations exist that match against the input data, that
    ...    input data is forwarded for remote processing" — the entity
    ...    lives on B2 only; B1's local scope stays empty.
    [Tags]    iop    iop-ext    5_6_1    4_3_6_3    since_v1.9.1
    ${ops}=    Evaluate    ["redirectionOps"]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect    operations=${ops}
    ${e}=    Simple Vehicle Entity    ${entity_id}    ${etype}    9
    ${response}=    Create Entity At Broker    ${b1_url}    ${e}
    Check Response Status Code    201    ${response.status_code}

    ${at_b2}=    Get Entity Via Broker    ${b2_url}    ${entity_id}
    Check Response Status Code    200    ${at_b2.status_code}
    Should Be Equal As Integers    ${at_b2.json()['speed']['value']}    9
    ${local}=    Query Entities Via Broker    ${b1_url}    type=${etype}    local=true
    Should Not Contain    ${local.text}    ${entity_id}

IOP_EXT_PRV_01_02 Create Against A Non-Supporting Redirect Is A Conflict
    [Documentation]    5.6.1.4: "For matching redirect Registrations where
    ...    the Create Entity operation is not supported, this shall result
    ...    in an error of type Conflict if the complete Create Entity
    ...    operation failed."
    [Tags]    iop    iop-ext    5_6_1    4_20    since_v1.9.1
    ${ops}=    Evaluate    ["queryEntity"]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect    operations=${ops}
    ${e}=    Simple Vehicle Entity    ${entity_id}    ${etype}    9
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST    url=${b1_url}/entities    json=${e}    headers=${headers}    expected_status=any
    Check Response Status Code    409    ${response.status_code}

    ${at_b2}=    Get Entity Via Broker    ${b2_url}    ${entity_id}
    Check Response Status Code    404    ${at_b2.status_code}
    ${at_b1}=    Get Entity Via Broker    ${b1_url}    ${entity_id}    local=true
    Check Response Status Code    404    ${at_b1.status_code}

IOP_EXT_PRV_01_03 Non-Supporting Inclusive Skips The Forward, Local Create Succeeds
    [Documentation]    5.6.1.4: inclusive registrations are forwarded to
    ...    only "in case the Create Entity operation is supported" — an
    ...    inclusive registration without it is silently skipped and the
    ...    entity is created locally.
    [Tags]    iop    iop-ext    5_6_1    4_20    since_v1.9.1
    ${ops}=    Evaluate    ["queryEntity"]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${ops}
    ${e}=    Simple Vehicle Entity    ${entity_id}    ${etype}    9
    ${response}=    Create Entity At Broker    ${b1_url}    ${e}
    Check Response Status Code    201    ${response.status_code}

    ${local}=    Get Entity Via Broker    ${b1_url}    ${entity_id}    local=true
    Check Response Status Code    200    ${local.status_code}
    ${at_b2}=    Get Entity Via Broker    ${b2_url}    ${entity_id}    local=true
    Check Response Status Code    404    ${at_b2.status_code}

IOP_EXT_PRV_01_04 A Forwarded Create Preserves Term Expansion
    [Documentation]    5.6.1.4 + 6.3.5: the forwarded create carries the
    ...    request @context — the remote broker stores the same expanded
    ...    IRI, never the bare term.
    [Tags]    iop    iop-ext    5_6_1    6_3_5    since_v1.9.1
    ${ops}=    Evaluate    ["redirectionOps"]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect    operations=${ops}
    ${e}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "velocidad": {"type": "Property", "value": 3}, "@context": ["https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld", {"velocidad": "https://example.org/vocab/spd"}]}
    &{headers}=    Create Dictionary    Content-Type=application/ld+json
    ${response}=    POST    url=${b1_url}/entities    json=${e}    headers=${headers}    expected_status=any
    Check Response Status Code    201    ${response.status_code}

    ${at_b2}=    Get Entity Via Broker    ${b2_url}    ${entity_id}
    Check Response Status Code    200    ${at_b2.status_code}
    Should Contain    ${at_b2.text}    example.org/vocab/spd
    Should Not Contain    ${at_b2.text}    velocidad

IOP_EXT_PRV_01_05 Update Attributes Split Across Both Brokers Succeeds
    [Documentation]    5.6.2.4: exclusive/redirect-matching Attributes are
    ...    forwarded and "then removed from the Fragment and not processed
    ...    further"; the remaining Attributes patch the local entity — both
    ...    halves applied, 204.
    [Tags]    iop    iop-ext    5_6_2    since_v1.9.1
    ${info}=    Evaluate
    ...    [{"entities": [{"id": $entity_id, "type": $etype}], "propertyNames": ["speed"]}]
    ${ops}=    Evaluate    ["redirectionOps"]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect    operations=${ops}    information=${info}
    ${remote}=    Simple Vehicle Entity    ${entity_id}    ${etype}    1
    Create Entity At Broker    ${b2_url}    ${remote}
    Create Local Entity At Broker    ${b1_url}
    ...    {"id": $entity_id, "type": $etype, "brandName": {"type": "Property", "value": "Skoda"}}

    ${fragment}=    Evaluate
    ...    {"speed": {"type": "Property", "value": 99}, "brandName": {"type": "Property", "value": "VW"}}
    ${response}=    Patch Entity Attrs Via Broker    ${b1_url}    ${entity_id}    ${fragment}
    Check Response Status Code    204    ${response.status_code}

    ${at_b2}=    Get Entity Via Broker    ${b2_url}    ${entity_id}
    Should Be Equal As Integers    ${at_b2.json()['speed']['value']}    99
    ${local}=    Get Entity Via Broker    ${b1_url}    ${entity_id}    local=true
    Should Be Equal    ${local.json()['brandName']['value']}    VW
    Dictionary Should Not Contain Key    ${local.json()}    speed

IOP_EXT_PRV_01_06 A Failed Remote Part Yields 207 Naming The Failed Attributes
    [Documentation]    5.6.2.4: a failing registration endpoint makes the
    ...    update "a partial success if some parts of the update succeeded"
    ...    — 207 with the failed Attribute in notUpdated and the local one
    ...    in updated (5.2.19).
    [Tags]    iop    iop-ext    5_6_2    5_2_19    since_v1.9.1
    ${info}=    Evaluate
    ...    [{"entities": [{"id": $entity_id, "type": $etype}], "propertyNames": ["speed"]}]
    ${ops}=    Evaluate    ["redirectionOps"]
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "mode": "redirect", "information": $info, "endpoint": $dead_endpoint, "operations": $ops}
    ${created}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${created.status_code}
    Create Local Entity At Broker    ${b1_url}
    ...    {"id": $entity_id, "type": $etype, "brandName": {"type": "Property", "value": "Skoda"}}

    ${fragment}=    Evaluate
    ...    {"speed": {"type": "Property", "value": 99}, "brandName": {"type": "Property", "value": "VW"}}
    ${response}=    Patch Entity Attrs Via Broker    ${b1_url}    ${entity_id}    ${fragment}
    Check Response Status Code    207    ${response.status_code}
    Should Contain    ${{ str($response.json()['updated']) }}    brandName
    Should Contain    ${{ str($response.json()['notUpdated']) }}    speed
    Should Not Contain    ${{ str($response.json()['updated']) }}    speed

IOP_EXT_PRV_01_07 Append Reaches The Remote Holder, Local Stays Empty
    [Documentation]    5.6.3.4: Attributes matching an exclusive/redirect
    ...    registration "are forwarded for remote processing" — the
    ...    appended attribute lands on B2; B1 holds nothing locally.
    [Tags]    iop    iop-ext    5_6_3    since_v1.9.1
    ${info}=    Evaluate
    ...    [{"entities": [{"id": $entity_id, "type": $etype}], "propertyNames": ["speed", "color"]}]
    ${ops}=    Evaluate    ["redirectionOps"]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect    operations=${ops}    information=${info}
    ${remote}=    Simple Vehicle Entity    ${entity_id}    ${etype}    1
    Create Entity At Broker    ${b2_url}    ${remote}

    ${fragment}=    Evaluate    {"color": {"type": "Property", "value": "red"}}
    ${response}=    Append Entity Attrs Via Broker    ${b1_url}    ${entity_id}    ${fragment}
    Should Be True    ${response.status_code} in (204, 207)

    ${at_b2}=    Get Entity Via Broker    ${b2_url}    ${entity_id}
    Should Be Equal    ${at_b2.json()['color']['value']}    red
    ${local}=    Get Entity Via Broker    ${b1_url}    ${entity_id}    local=true
    Check Response Status Code    404    ${local.status_code}

IOP_EXT_PRV_01_08 Partial Update Forwards; An Unknown Attribute Is 404 Everywhere
    [Documentation]    5.6.4.4: matching input is forwarded to the redirect
    ...    holder; "If the target Entity does not contain the target
    ...    Attribute ... then an error of type ResourceNotFound shall be
    ...    raised."
    [Tags]    iop    iop-ext    5_6_4    since_v1.9.1
    ${info}=    Evaluate
    ...    [{"entities": [{"id": $entity_id, "type": $etype}], "propertyNames": ["speed"]}]
    ${ops}=    Evaluate    ["redirectionOps"]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect    operations=${ops}    information=${info}
    ${remote}=    Simple Vehicle Entity    ${entity_id}    ${etype}    1
    Create Entity At Broker    ${b2_url}    ${remote}

    &{headers}=    Create Dictionary    Content-Type=application/json
    ${eid}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($entity_id, safe='')
    ${fragment}=    Evaluate    {"value": 55}
    ${response}=    PATCH    url=${b1_url}/entities/${eid}/attrs/speed    json=${fragment}
    ...    headers=${headers}    expected_status=any
    Check Response Status Code    204    ${response.status_code}
    ${at_b2}=    Get Entity Via Broker    ${b2_url}    ${entity_id}
    Should Be Equal As Integers    ${at_b2.json()['speed']['value']}    55

    ${response}=    PATCH    url=${b1_url}/entities/${eid}/attrs/nonexistent    json=${fragment}
    ...    headers=${headers}    expected_status=any
    Check Response Status Code    404    ${response.status_code}
    ${at_b2}=    Get Entity Via Broker    ${b2_url}    ${entity_id}
    Should Be Equal As Integers    ${at_b2.json()['speed']['value']}    55


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${suffix}
    Set Test Variable    ${etype}    IopPrv${suffix}
    Set Test Variable    ${entity_id}    urn:ngsi-ld:IopPrv:${suffix}
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:iopprv-${suffix}

Create Local Entity At Broker
    [Documentation]    POST /entities?local=true — 6.3.18 local scope, no
    ...    registrations considered.
    [Arguments]    ${at}    ${entity_expr}
    ${entity}=    Evaluate    ${entity_expr}
    &{headers}=    Create Dictionary    Content-Type=application/json
    &{params}=    Create Dictionary    local=true
    ${response}=    POST    url=${at}/entities    json=${entity}    params=${params}
    ...    headers=${headers}    expected_status=any
    Should Be Equal As Integers    ${response.status_code}    201
    RETURN    ${response}

Cleanup Interop Fixtures
    Delete Registration At Broker    ${b1_url}    ${registration_id}
    FOR    ${at}    IN    ${b1_url}    ${b2_url}    ${b3_url}
        Delete Entity Via Broker    ${at}    ${entity_id}
    END
