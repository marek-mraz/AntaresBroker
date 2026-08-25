*** Settings ***
Documentation       Distributed provision II (Antares extension IOP TPs).
...                 5.6.6.4: delete forwarded to matching exclusive/redirect
...                 registrations, "The input data shall be used to remove
...                 the entity locally if it exists"; the optional 4.17 type
...                 selector gates the target. 5.6.5.4: delete Attribute
...                 forwarded to the holding registration. 5.6.7.4 batch
...                 create: per-CSR input arrays, non-matching items
...                 removed; S/E arrays per 5.2.16/5.2.17. 5.6.8.4 upsert:
...                 without upsertBatch support the update-mode falls back
...                 to forwarded Update Attributes. 5.6.10.4 batch delete:
...                 remote errors merge into E, local deletes into S.
...                 5.6.17.4 merge: matching Attributes forwarded, "then
...                 removed from the Fragment and not processed further".

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
IOP_EXT_PRV_02_01 Delete Removes Both The Local And The Redirect Half
    [Documentation]    5.6.6.4: the delete is forwarded to the matching
    ...    redirect registration AND "the input data shall be used to
    ...    remove the entity locally if it exists" — both halves gone,
    ...    re-retrieve 404.
    [Tags]    iop    iop-ext    5_6_6    since_v1.9.1
    ${info}=    Evaluate
    ...    [{"entities": [{"id": $entity_id, "type": $etype}], "propertyNames": ["speed"]}]
    ${ops}=    Evaluate    ["redirectionOps"]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect    operations=${ops}    information=${info}
    ${remote}=    Simple Vehicle Entity    ${entity_id}    ${etype}    1
    Create Entity At Broker    ${b2_url}    ${remote}
    Create Local Entity At Broker    ${b1_url}
    ...    {"id": $entity_id, "type": $etype, "brandName": {"type": "Property", "value": "Skoda"}}

    ${response}=    Delete Entity Via Broker    ${b1_url}    ${entity_id}
    Should Be True    ${response.status_code} in (204, 207)

    ${at_b2}=    Get Entity Via Broker    ${b2_url}    ${entity_id}
    Check Response Status Code    404    ${at_b2.status_code}
    ${local}=    Get Entity Via Broker    ${b1_url}    ${entity_id}    local=true
    Check Response Status Code    404    ${local.status_code}
    ${refetch}=    Get Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    404    ${refetch.status_code}

IOP_EXT_PRV_02_02 Delete With A Non-Matching Type Selector Deletes Nothing
    [Documentation]    4.17 + 5.6.6.4: "no existing Entity whose id (URI),
    ...    and where specified type, is equivalent" → ResourceNotFound; the
    ...    non-matching selector must leave both halves untouched.
    [Tags]    iop    iop-ext    5_6_6    4_17    since_v1.9.1
    ${info}=    Evaluate
    ...    [{"entities": [{"id": $entity_id, "type": $etype}], "propertyNames": ["speed"]}]
    ${ops}=    Evaluate    ["redirectionOps"]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect    operations=${ops}    information=${info}
    ${remote}=    Simple Vehicle Entity    ${entity_id}    ${etype}    1
    Create Entity At Broker    ${b2_url}    ${remote}
    Create Local Entity At Broker    ${b1_url}
    ...    {"id": $entity_id, "type": $etype, "brandName": {"type": "Property", "value": "Skoda"}}

    ${eid}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($entity_id, safe='')
    &{params}=    Create Dictionary    type=SomeOtherType
    ${response}=    DELETE    url=${b1_url}/entities/${eid}    params=${params}    expected_status=any
    Check Response Status Code    404    ${response.status_code}

    ${at_b2}=    Get Entity Via Broker    ${b2_url}    ${entity_id}
    Check Response Status Code    200    ${at_b2.status_code}
    ${local}=    Get Entity Via Broker    ${b1_url}    ${entity_id}    local=true
    Check Response Status Code    200    ${local.status_code}

IOP_EXT_PRV_02_03 deleteAttribute Of A Remote-Only Attribute Forwards
    [Documentation]    5.6.5.4: input matching an exclusive/redirect
    ...    registration "is forwarded to the Registration endpoint" — the
    ...    remote attribute disappears, the local one is untouched.
    [Tags]    iop    iop-ext    5_6_5    since_v1.9.1
    ${info}=    Evaluate
    ...    [{"entities": [{"id": $entity_id, "type": $etype}], "propertyNames": ["speed"]}]
    ${ops}=    Evaluate    ["redirectionOps"]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect    operations=${ops}    information=${info}
    ${remote}=    Simple Vehicle Entity    ${entity_id}    ${etype}    1
    Create Entity At Broker    ${b2_url}    ${remote}
    Create Local Entity At Broker    ${b1_url}
    ...    {"id": $entity_id, "type": $etype, "brandName": {"type": "Property", "value": "Skoda"}}

    ${response}=    Delete Entity Attr Via Broker    ${b1_url}    ${entity_id}    speed
    Should Be True    ${response.status_code} in (204, 207)

    ${at_b2}=    Get Entity Via Broker    ${b2_url}    ${entity_id}
    Dictionary Should Not Contain Key    ${at_b2.json()}    speed
    ${local}=    Get Entity Via Broker    ${b1_url}    ${entity_id}    local=true
    Should Be Equal    ${local.json()['brandName']['value']}    Skoda

IOP_EXT_PRV_02_04 Batch Create Routes Each Item To Its Redirect Source
    [Documentation]    5.6.7.4: per CSR, "Remove from IN all Entities not
    ...    matched by CSR"; without createBatch support the broker falls
    ...    back to forwarded Create Entity per item — each entity lands on
    ...    its own broker.
    [Tags]    iop    iop-ext    5_6_7    since_v1.9.1
    ${ops}=    Evaluate    ["redirectionOps"]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}A
    ...    mode=redirect    operations=${ops}
    Register Broker As Context Source    ${b1_url}    ${registration_id}-3    ${b3_url}    ${etype}B
    ...    mode=redirect    operations=${ops}
    ${batch}=    Evaluate
    ...    [{"id": $entity_id + "-a", "type": $etype + "A", "speed": {"type": "Property", "value": 1}}, {"id": $entity_id + "-b", "type": $etype + "B", "speed": {"type": "Property", "value": 2}}]

    ${response}=    Batch Op Via Broker    ${b1_url}    create    ${batch}
    Should Be True    ${response.status_code} in (201, 207)

    ${b2_has}=    Get Entity Via Broker    ${b2_url}    ${entity_id}-a
    Check Response Status Code    200    ${b2_has.status_code}
    ${b2_not}=    Get Entity Via Broker    ${b2_url}    ${entity_id}-b    local=true
    Check Response Status Code    404    ${b2_not.status_code}
    ${b3_has}=    Get Entity Via Broker    ${b3_url}    ${entity_id}-b
    Check Response Status Code    200    ${b3_has.status_code}
    ${b3_not}=    Get Entity Via Broker    ${b3_url}    ${entity_id}-a    local=true
    Check Response Status Code    404    ${b3_not.status_code}

IOP_EXT_PRV_02_05 Batch Create Splits Success And Errors Correctly
    [Documentation]    5.6.7.4/5.2.16: one item redirected to a dead source
    ...    fails, the plain local item succeeds — the
    ...    BatchOperationResult's success[] holds only the local id and
    ...    errors[] names the failed one.
    [Tags]    iop    iop-ext    5_6_7    5_2_16    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype + "A"}]}]
    ${ops}=    Evaluate    ["createEntity"]
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "mode": "redirect", "information": $info, "endpoint": $dead_endpoint, "operations": $ops}
    ${created}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${created.status_code}
    ${batch}=    Evaluate
    ...    [{"id": $entity_id + "-a", "type": $etype + "A", "speed": {"type": "Property", "value": 1}}, {"id": $entity_id + "-l", "type": $etype, "speed": {"type": "Property", "value": 2}}]

    ${response}=    Batch Op Via Broker    ${b1_url}    create    ${batch}
    Check Response Status Code    207    ${response.status_code}
    ${body}=    Evaluate    $response.json()
    Should Contain    ${{ str($body['success']) }}    ${entity_id}-l
    Should Contain    ${{ str($body['errors']) }}    ${entity_id}-a
    Should Not Contain    ${{ str($body['success']) }}    ${entity_id}-a

IOP_EXT_PRV_02_06 Batch Upsert Update-Mode Falls Back To Forwarded Update
    [Documentation]    5.6.8.4: without upsertBatch support, "if the Update
    ...    Attributes operation (clause 5.6.2) is supported by CSR and the
    ...    value of the update mode flag is Update: Forward an Update
    ...    Attributes request" — the redirect-held entity is updated on B2.
    [Tags]    iop    iop-ext    5_6_8    since_v1.9.1
    ${info}=    Evaluate
    ...    [{"entities": [{"id": $entity_id, "type": $etype}], "propertyNames": ["speed"]}]
    ${ops}=    Evaluate    ["redirectionOps"]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect    operations=${ops}    information=${info}
    ${remote}=    Simple Vehicle Entity    ${entity_id}    ${etype}    1
    Create Entity At Broker    ${b2_url}    ${remote}
    ${batch}=    Evaluate
    ...    [{"id": $entity_id, "type": $etype, "speed": {"type": "Property", "value": 77}}]

    ${response}=    Batch Op Via Broker    ${b1_url}    upsert    ${batch}    options=update
    Should Be True    ${response.status_code} in (204, 201, 207)

    ${at_b2}=    Get Entity Via Broker    ${b2_url}    ${entity_id}
    Should Be Equal As Integers    ${at_b2.json()['speed']['value']}    77

IOP_EXT_PRV_02_07 Batch Delete Reports The Remote Miss And Deletes Local Items
    [Documentation]    5.6.10.4: forwarded per-item deletes merge "any
    ...    error result(s) ... with E" while local deletions land in S —
    ...    the remote 404 item is listed in errors[], the local item is
    ...    gone.
    [Tags]    iop    iop-ext    5_6_10    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"id": $entity_id + "-a", "type": $etype + "A"}]}]
    ${ops}=    Evaluate    ["redirectionOps"]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}A
    ...    mode=redirect    operations=${ops}    information=${info}
    ${local}=    Simple Vehicle Entity    ${entity_id}-l    ${etype}    2
    Create Entity At Broker    ${b1_url}    ${local}
    ${batch}=    Evaluate    [$entity_id + "-a", $entity_id + "-l"]

    ${response}=    Batch Op Via Broker    ${b1_url}    delete    ${batch}
    Check Response Status Code    207    ${response.status_code}
    ${body}=    Evaluate    $response.json()
    Should Contain    ${{ str($body['success']) }}    ${entity_id}-l
    Should Contain    ${{ str($body['errors']) }}    ${entity_id}-a
    ${gone}=    Get Entity Via Broker    ${b1_url}    ${entity_id}-l    local=true
    Check Response Status Code    404    ${gone.status_code}

IOP_EXT_PRV_02_08 Merge Entity Forwards The Redirect-Held Fragment Part
    [Documentation]    5.6.17.4: "If the Merge Entity operation is supported
    ...    by the matched registration ... matching input data is forwarded
    ...    to the Registration endpoint. The matching Attributes are then
    ...    removed from the Fragment and not processed further" — the
    ...    remote half merges on B2, the local half on B1.
    [Tags]    iop    iop-ext    5_6_17    since_v1.9.1
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
    ...    {"speed": {"type": "Property", "value": 88}, "brandName": {"type": "Property", "value": "VW"}}
    ${response}=    Merge Entity Via Broker    ${b1_url}    ${entity_id}    ${fragment}
    Should Be True    ${response.status_code} in (204, 207)

    ${at_b2}=    Get Entity Via Broker    ${b2_url}    ${entity_id}
    Should Be Equal As Integers    ${at_b2.json()['speed']['value']}    88
    ${local}=    Get Entity Via Broker    ${b1_url}    ${entity_id}    local=true
    Should Be Equal    ${local.json()['brandName']['value']}    VW
    Dictionary Should Not Contain Key    ${local.json()}    speed


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${suffix}
    Set Test Variable    ${etype}    IopPrw${suffix}
    Set Test Variable    ${entity_id}    urn:ngsi-ld:IopPrw:${suffix}
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:iopprw-${suffix}

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
    Delete Registration At Broker    ${b1_url}    ${registration_id}-3
    FOR    ${tail}    IN    ${EMPTY}    -a    -b    -l
        Delete Entity Via Broker    ${b1_url}    ${entity_id}${tail}
        Delete Entity Via Broker    ${b2_url}    ${entity_id}${tail}
        Delete Entity Via Broker    ${b3_url}    ${entity_id}${tail}
    END
