*** Settings ***
Documentation       4.22 × Temporal Evolution (Antares extension TP). What creates
...                 history and what expiry does to it: (a) every attribute-value
...                 write (create/partial update) appends a temporal instance —
...                 the 5.6.x behaviours' "temporal representation shall be
...                 updated"; (b) an expiresAt-only merge changes NO attribute so
...                 it appends NOTHING; (c) a transient attribute's instances carry
...                 its expiresAt (5.2.5: expiry of "the storage of the Property"),
...                 so at expiry its history vanishes from temporal reads while the
...                 durable sibling's history stays; (d) entity-level expiry kills
...                 the CURRENT state only — the Temporal Evolution survives,
...                 mirroring Delete Entity (5.6.6) semantics where history also
...                 outlives the entity (CIM 009 is silent on cascading expiry to
...                 the temporal evolution; the delete analogy is the documented
...                 Antares reading).

Library             DateTime
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Teardown      Cleanup Entities


*** Variables ***
${EXPIRY_SECONDS}       ${5}
${TEMPORAL_QS}          timerel=before&timeAt=2030-01-01T00:00:00Z&timeproperty=modifiedAt&options=temporalValues


*** Test Cases ***
422_07_01 Attribute Writes Append History ExpiresAt Alone Does Not
    [Tags]    transient    4_22    troe
    ${eid_hist}=    Generate Random Building Entity Id
    Set Suite Variable    ${eid_hist}
    ${entity}=    Evaluate
    ...    {"id": $eid_hist, "type": "Building", "temperature": {"type": "Property", "value": 1}, "@context": $ngsild_test_suite_context}
    ${response}=    Create Entity From JSON-LD Content    ${entity}
    Check Response Status Code    201    ${response.status_code}
    Patch Attr    ${eid_hist}    temperature    2
    Patch Attr    ${eid_hist}    temperature    3
    ${values}=    Temporal Values Of    ${eid_hist}    temperature
    ${nums}=    Evaluate    sorted(v[0] for v in $values)
    Should Be Equal    ${nums}    ${{[1, 2, 3]}}    create + 2 updates = exactly these 3 instances
    # expiresAt-only merge: no attribute changed => no new instance
    &{headers}=    Create Dictionary    Content-Type=application/ld+json
    ${fragment}=    Evaluate
    ...    {"id": $eid_hist, "type": "Building", "expiresAt": "2030-01-01T00:00:00Z", "@context": $ngsild_test_suite_context}
    ${response}=    PATCH
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${eid_hist}
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    204    ${response.status_code}
    ${values}=    Temporal Values Of    ${eid_hist}    temperature
    ${nums}=    Evaluate    sorted(v[0] for v in $values)
    Should Be Equal    ${nums}    ${{[1, 2, 3]}}    expiresAt is meta, not an attribute — history unchanged

422_07_02 Transient Attribute History Evaporates Durable History Stays
    [Tags]    transient    4_22    troe
    ${eid_attr}=    Generate Random Building Entity Id
    Set Suite Variable    ${eid_attr}
    ${now}=    Get Current Date    time_zone=UTC
    ${expiry}=    Add Time To Date    ${now}    ${EXPIRY_SECONDS}s    result_format=%Y-%m-%dT%H:%M:%SZ
    ${payload}=    Catenate    SEPARATOR=${SPACE}
    ...    {"id": "${eid_attr}", "type": "Building",
    ...    "flash": {"type": "Property", "value": 10, "expiresAt": "${expiry}"},
    ...    "steady": {"type": "Property", "value": 100},
    ...    "@context": "${ngsild_test_suite_context}"}
    ${entity}=    Evaluate    json.loads($payload)    modules=json
    ${response}=    Create Entity From JSON-LD Content    ${entity}
    Check Response Status Code    201    ${response.status_code}
    # 5.6.4 partial update keeps flash's expiresAt (5.5.8) — its new instance
    # is transient too
    Patch Attr    ${eid_attr}    flash    11
    ${values}=    Temporal Values Of    ${eid_attr}    flash
    ${nums}=    Evaluate    sorted(v[0] for v in $values)
    Should Be Equal    ${nums}    ${{[10, 11]}}    both flash instances recorded pre-expiry
    Sleep    ${EXPIRY_SECONDS + 2}s
    ${response}=    Temporal Retrieve    ${eid_attr}
    Check Response Status Code    200    ${response.status_code}
    ${body}=    Evaluate    $response.json()
    ${keys}=    Evaluate    sorted($body.keys())
    Should Not Contain    ${keys}    flash    the transient attribute's history evaporated
    ${steady}=    Evaluate    [v[0] for v in $body["steady"]["values"]]
    Should Be Equal    ${steady}    ${{[100]}}    durable history intact, exact values

422_07_03 Entity Expiry Kills Current State But History Survives
    [Tags]    transient    4_22    troe
    ${eid_ent}=    Generate Random Building Entity Id
    Set Suite Variable    ${eid_ent}
    ${now}=    Get Current Date    time_zone=UTC
    ${expiry}=    Add Time To Date    ${now}    ${EXPIRY_SECONDS}s    result_format=%Y-%m-%dT%H:%M:%SZ
    ${payload}=    Catenate    SEPARATOR=${SPACE}
    ...    {"id": "${eid_ent}", "type": "Building",
    ...    "temperature": {"type": "Property", "value": 7},
    ...    "expiresAt": "${expiry}",
    ...    "@context": "${ngsild_test_suite_context}"}
    ${entity}=    Evaluate    json.loads($payload)    modules=json
    ${response}=    Create Entity From JSON-LD Content    ${entity}
    Check Response Status Code    201    ${response.status_code}
    Patch Attr    ${eid_ent}    temperature    8
    Sleep    ${EXPIRY_SECONDS + 2}s
    ${response}=    Retrieve Entity    id=${eid_ent}    context=${ngsild_test_suite_context}
    Check Response Status Code    404    ${response.status_code}
    ${response}=    Temporal Retrieve    ${eid_ent}
    Check Response Status Code    200    ${response.status_code}
    ${values}=    Temporal Values Of    ${eid_ent}    temperature
    ${nums}=    Evaluate    sorted(v[0] for v in $values)
    Should Be Equal    ${nums}    ${{[7, 8]}}    the attribute's full history outlives the entity


*** Keywords ***
Patch Attr
    [Arguments]    ${id}    ${attr}    ${value}
    &{headers}=    Create Dictionary    Content-Type=application/ld+json
    # int(): robot arguments are strings — sending $value raw would store a
    # JSON string and corrupt the exact-value history assertions
    ${fragment}=    Evaluate
    ...    {"type": "Property", "value": int($value), "@context": $ngsild_test_suite_context}
    ${response}=    PATCH
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${id}/attrs/${attr}
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    204    ${response.status_code}

Temporal Retrieve
    [Arguments]    ${id}
    &{headers}=    Create Dictionary    Accept=application/ld+json
    ${response}=    GET
    ...    url=${url}/${TEMPORAL_ENTITIES_ENDPOINT_PATH}/${id}?${TEMPORAL_QS}
    ...    headers=${headers}
    ...    expected_status=any
    RETURN    ${response}

Temporal Values Of
    [Arguments]    ${id}    ${attr}
    ${response}=    Temporal Retrieve    ${id}
    Check Response Status Code    200    ${response.status_code}
    ${values}=    Evaluate    $response.json().get($attr, {}).get("values", [])
    RETURN    ${values}

Cleanup Entities
    Delete Entity    ${eid_hist}
    Delete Entity    ${eid_attr}
    Delete Entity    ${eid_ent}
