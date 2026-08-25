*** Settings ***
Documentation       Verify the 4.5.6 temporal representation of the Scope after a Core API change.
...
...                 Clause 4.5.6: "the Scope (if present) shall be represented as
...                 a temporal representation of a Property (clause 4.5.7) that can
...                 only have the non-reified Temporal Properties createdAt,
...                 modifiedAt, deletedAt and observedAt as sub-Properties. ... In
...                 case the Temporal Evolution of the Scope is updated as the
...                 result of a change from the Core API, the observedAt
...                 sub-Property should be set as a copy of the modifiedAt
...                 sub-Property."
...
...                 Antares extension TP — official coverage (020_19/020_20) only
...                 exercises scope deletion, not the Core-API update path.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Teardown       Delete Entity    ${entity_id}


*** Test Cases ***
456_01 Scope Update From Core API Becomes A Temporal Property Instance
    [Documentation]    4.5.6: after a Core API scope change the temporal scope is
    ...    an instance array of Property with observedAt = copy of modifiedAt and
    ...    no sub-properties beyond the non-reified temporal ones.
    [Tags]    te-retrieve    4_5_6    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "scope": "/Old", "speed": {"type": "Property", "value": 1, "observedAt": "2026-08-10T00:00:00Z"}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${fragment}=    Evaluate    {"type": "Vehicle", "scope": "/New"}
    ${response}=    PATCH
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}
    ...    headers=${headers}
    ...    json=${fragment}
    ...    expected_status=any
    Check Response Status Code    204    ${response.status_code}
    ${response}=    Retrieve Temporal Representation Of Entity
    ...    temporal_entity_representation_id=${entity_id}
    ...    options=sysAttrs
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${body}=    Set Variable    ${response.json()}
    ${scope_instances}=    Evaluate    [i for i in ($body['scope'] if isinstance($body['scope'], list) else [$body['scope']]) if isinstance(i, dict)]
    Should Not Be Empty    ${scope_instances}
    ${new_inst}=    Evaluate    [i for i in $scope_instances if i.get('value') in ('/New', ['/New'])]
    Length Should Be    ${new_inst}    1
    ${inst_type}=    Evaluate    $new_inst[0]['type']
    Should Be Equal    ${inst_type}    Property
    ${observed}=    Evaluate    $new_inst[0]['observedAt']
    ${modified}=    Evaluate    $new_inst[0]['modifiedAt']
    Should Be Equal    ${observed}    ${modified}
    ${illegal}=    Evaluate    [k for k in $new_inst[0] if k not in ('type', 'value', 'instanceId', 'createdAt', 'modifiedAt', 'deletedAt', 'observedAt')]
    Should Be Empty    ${illegal}
