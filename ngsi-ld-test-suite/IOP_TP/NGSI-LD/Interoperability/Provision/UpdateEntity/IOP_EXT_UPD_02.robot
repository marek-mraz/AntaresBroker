*** Settings ***
Documentation       Provision variants ported from the single-broker TP tree
...                 to real two-broker setups (Antares extension IOP TPs):
...                 merge/replace entity, replace attribute, the three batch
...                 write operations, purge, datasetId-instance deletion,
...                 noOverwrite append and the op-support Conflict gate —
...                 each issued at B1 and asserted DIRECTLY in B2.

Resource            ${EXECDIR}/resources/ApiUtils/InteropUtils.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Library             Collections

Test Setup          Setup Interop Ids
Test Teardown       Cleanup Interop Fixtures


*** Variables ***
${b1_url}
${b2_url}
${WRITE_OPS}=       ${{ ["createEntity", "updateEntity", "appendAttrs", "updateAttrs", "deleteAttrs", "deleteEntity", "mergeEntity", "replaceEntity", "replaceAttrs", "purgeEntity", "createBatch", "upsertBatch", "updateBatch", "deleteBatch", "retrieveEntity", "queryEntity"] }}
${READ_OPS}=        ${{ ["retrieveEntity", "queryEntity"] }}


*** Test Cases ***
IOP_EXT_UPD_02_01 Merge Entity Via Broker1 Reaches Broker2
    [Documentation]    5.6.17 through 4.3.6: PATCH /entities/{id} at B1 —
    ...    B2's copy gains the new Attribute and the changed value, existing
    ...    Attributes survive (merge, not replace).
    [Tags]    iop    iop-ext    5_6_17    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect    operations=${WRITE_OPS}
    ${entity}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": {"type": "Property", "value": 42}, "brandName": {"type": "Property", "value": "Mercedes"}}
    Create Entity At Broker    ${b2_url}    ${entity}

    ${fragment}=    Evaluate
    ...    {"type": $etype, "speed": {"type": "Property", "value": 77}, "color": {"type": "Property", "value": "red"}}
    ${response}=    Merge Entity Via Broker    ${b1_url}    ${entity_id}    ${fragment}
    Should Be True    ${response.status_code} in (204, 207)

    ${response}=    Get Entity Via Broker    ${b2_url}    ${entity_id}    local=true
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal As Integers    ${response.json()['speed']['value']}    77
    Dictionary Should Contain Key    ${response.json()}    color
    Dictionary Should Contain Key    ${response.json()}    brandName

IOP_EXT_UPD_02_02 Replace Entity Via Broker1 Replaces Broker2's Copy
    [Documentation]    5.6.18 through 4.3.6: PUT /entities/{id} at B1 — the
    ...    old brandName is GONE in B2 (replace, not merge).
    [Tags]    iop    iop-ext    5_6_18    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect    operations=${WRITE_OPS}
    ${entity}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": {"type": "Property", "value": 42}, "brandName": {"type": "Property", "value": "Mercedes"}}
    Create Entity At Broker    ${b2_url}    ${entity}

    ${replacement}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": {"type": "Property", "value": 1}}
    ${response}=    Replace Entity Via Broker    ${b1_url}    ${entity_id}    ${replacement}
    Should Be True    ${response.status_code} in (204, 207)

    ${response}=    Get Entity Via Broker    ${b2_url}    ${entity_id}    local=true
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal As Integers    ${response.json()['speed']['value']}    1
    Dictionary Should Not Contain Key    ${response.json()}    brandName

IOP_EXT_UPD_02_03 Replace Attribute Via Broker1
    [Documentation]    5.6.19 through 4.3.6: PUT /entities/{id}/attrs/speed
    ...    at B1 replaces the Attribute in B2.
    [Tags]    iop    iop-ext    5_6_19    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect    operations=${WRITE_OPS}
    ${entity}=    Simple Vehicle Entity    ${entity_id}    ${etype}    42
    Create Entity At Broker    ${b2_url}    ${entity}

    ${fragment}=    Evaluate    {"type": "Property", "value": 55}
    ${response}=    Replace Attr Via Broker    ${b1_url}    ${entity_id}    speed    ${fragment}
    Should Be True    ${response.status_code} in (204, 207)

    ${response}=    Get Entity Via Broker    ${b2_url}    ${entity_id}    local=true
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal As Integers    ${response.json()['speed']['value']}    55

IOP_EXT_UPD_02_04 Batch Create Via Broker1 Lands In Broker2
    [Documentation]    5.6.7 through 4.3.6: both batch items match the
    ...    redirect registration — both entities exist in B2, neither in B1.
    [Tags]    iop    iop-ext    5_6_7    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect    operations=${WRITE_OPS}
    ${batch}=    Evaluate
    ...    [{"id": $entity_id + "-1", "type": $etype, "speed": {"type": "Property", "value": 1}}, {"id": $entity_id + "-2", "type": $etype, "speed": {"type": "Property", "value": 2}}]
    ${response}=    Batch Op Via Broker    ${b1_url}    create    ${batch}
    Should Be True    ${response.status_code} in (201, 204, 207)

    ${response}=    Get Entity Via Broker    ${b2_url}    ${entity_id}-1    local=true
    Check Response Status Code    200    ${response.status_code}
    ${response}=    Get Entity Via Broker    ${b2_url}    ${entity_id}-2    local=true
    Check Response Status Code    200    ${response.status_code}
    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}-1    local=true
    Check Response Status Code    404    ${response.status_code}

IOP_EXT_UPD_02_05 Batch Upsert Via Broker1 Updates And Creates In Broker2
    [Documentation]    5.6.8 through 4.3.6: one existing entity is updated,
    ...    one new entity is created — both changes land in B2.
    [Tags]    iop    iop-ext    5_6_8    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect    operations=${WRITE_OPS}
    ${existing}=    Simple Vehicle Entity    ${entity_id}-1    ${etype}    1
    Create Entity At Broker    ${b2_url}    ${existing}

    ${batch}=    Evaluate
    ...    [{"id": $entity_id + "-1", "type": $etype, "speed": {"type": "Property", "value": 11}}, {"id": $entity_id + "-2", "type": $etype, "speed": {"type": "Property", "value": 22}}]
    ${response}=    Batch Op Via Broker    ${b1_url}    upsert    ${batch}
    Should Be True    ${response.status_code} in (201, 204, 207)

    ${response}=    Get Entity Via Broker    ${b2_url}    ${entity_id}-1    local=true
    Should Be Equal As Integers    ${response.json()['speed']['value']}    11
    ${response}=    Get Entity Via Broker    ${b2_url}    ${entity_id}-2    local=true
    Check Response Status Code    200    ${response.status_code}

IOP_EXT_UPD_02_06 Batch Delete Via Broker1 Empties Broker2
    [Documentation]    5.6.10 through 4.3.6: both listed entities disappear
    ...    from B2.
    [Tags]    iop    iop-ext    5_6_10    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect    operations=${WRITE_OPS}
    FOR    ${i}    IN    1    2
        ${e}=    Simple Vehicle Entity    ${entity_id}-${i}    ${etype}    ${i}
        Create Entity At Broker    ${b2_url}    ${e}
    END

    ${batch}=    Evaluate    [$entity_id + "-1", $entity_id + "-2"]
    ${response}=    Batch Op Via Broker    ${b1_url}    delete    ${batch}
    Should Be True    ${response.status_code} in (204, 207)

    ${response}=    Get Entity Via Broker    ${b2_url}    ${entity_id}-1    local=true
    Check Response Status Code    404    ${response.status_code}
    ${response}=    Get Entity Via Broker    ${b2_url}    ${entity_id}-2    local=true
    Check Response Status Code    404    ${response.status_code}

IOP_EXT_UPD_02_07 Purge Via Broker1 Clears Broker2's Matching Entities
    [Documentation]    5.6.21 through 4.3.6: DELETE /entities?type=… at B1
    ...    purges the matching entities in B2; a different-type entity
    ...    survives.
    [Tags]    iop    iop-ext    5_6_21    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect    operations=${WRITE_OPS}
    ${victim}=    Simple Vehicle Entity    ${entity_id}-v    ${etype}    1
    Create Entity At Broker    ${b2_url}    ${victim}
    ${bystander}=    Evaluate
    ...    {"id": $entity_id + "-b", "type": $etype + "Other", "speed": {"type": "Property", "value": 1}}
    Create Entity At Broker    ${b2_url}    ${bystander}

    ${response}=    Purge Entities Via Broker    ${b1_url}    type=${etype}
    Should Be True    ${response.status_code} in (204, 207)

    ${response}=    Get Entity Via Broker    ${b2_url}    ${entity_id}-v    local=true
    Check Response Status Code    404    ${response.status_code}
    ${response}=    Get Entity Via Broker    ${b2_url}    ${entity_id}-b    local=true
    Check Response Status Code    200    ${response.status_code}

IOP_EXT_UPD_02_08 A Read-Only Registration Refuses The Write
    [Documentation]    5.6.2.4/4.3.6: the redirect registration supports
    ...    only reads — PATCH attrs via B1 must fail with Conflict (409/207)
    ...    and B2's value must be UNCHANGED.
    [Tags]    iop    iop-ext    5_6_2    4_20    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect    operations=${READ_OPS}
    ${entity}=    Simple Vehicle Entity    ${entity_id}    ${etype}    42
    Create Entity At Broker    ${b2_url}    ${entity}

    ${fragment}=    Evaluate    {"speed": {"type": "Property", "value": 77}}
    ${response}=    Patch Entity Attrs Via Broker    ${b1_url}    ${entity_id}    ${fragment}
    Should Be True    ${response.status_code} in (207, 404, 409)

    ${response}=    Get Entity Via Broker    ${b2_url}    ${entity_id}    local=true
    Should Be Equal As Integers    ${response.json()['speed']['value']}    42

IOP_EXT_UPD_02_09 Deleting A DatasetId Instance Via Broker1
    [Documentation]    5.6.5 datasetId: only the named instance disappears
    ...    from B2; the default instance survives.
    [Tags]    iop    iop-ext    5_6_5    4_5_5    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect    operations=${WRITE_OPS}
    ${entity}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 10}, {"type": "Property", "value": 99, "datasetId": "urn:ngsi-ld:Dataset:gps"}]}
    Create Entity At Broker    ${b2_url}    ${entity}

    ${eid}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($entity_id, safe='')
    ${response}=    DELETE
    ...    url=${b1_url}/entities/${eid}/attrs/speed
    ...    params=datasetId=urn:ngsi-ld:Dataset:gps
    ...    expected_status=any
    Should Be True    ${response.status_code} in (204, 207)

    ${response}=    Get Entity Via Broker    ${b2_url}    ${entity_id}    local=true
    Check Response Status Code    200    ${response.status_code}
    Dictionary Should Contain Key    ${response.json()}    speed
    Should Not Contain    ${response.text}    urn:ngsi-ld:Dataset:gps

IOP_EXT_UPD_02_10 NoOverwrite Append Via Broker1 Preserves Broker2's Value
    [Documentation]    5.6.3 options=noOverwrite: appending an Attribute
    ...    that already exists in B2 must NOT overwrite it; a genuinely new
    ...    Attribute is still appended.
    [Tags]    iop    iop-ext    5_6_3    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect    operations=${WRITE_OPS}
    ${entity}=    Simple Vehicle Entity    ${entity_id}    ${etype}    42
    Create Entity At Broker    ${b2_url}    ${entity}

    ${fragment}=    Evaluate
    ...    {"speed": {"type": "Property", "value": 1}, "color": {"type": "Property", "value": "red"}}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${eid}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($entity_id, safe='')
    ${response}=    POST
    ...    url=${b1_url}/entities/${eid}/attrs
    ...    params=options=noOverwrite
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Should Be True    ${response.status_code} in (204, 207)

    ${response}=    Get Entity Via Broker    ${b2_url}    ${entity_id}    local=true
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal As Integers    ${response.json()['speed']['value']}    42
    Dictionary Should Contain Key    ${response.json()}    color


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${etype}    IopVeh${suffix}
    Set Test Variable    ${entity_id}    urn:ngsi-ld:IopVeh:${suffix}
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:iopext-${suffix}

Cleanup Interop Fixtures
    FOR    ${tail}    IN    ${EMPTY}    -1    -2    -v    -b
        Delete Entity Via Broker    ${b1_url}    ${entity_id}${tail}
        Delete Entity Via Broker    ${b2_url}    ${entity_id}${tail}
    END
    Delete Registration At Broker    ${b1_url}    ${registration_id}
