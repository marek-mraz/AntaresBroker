*** Settings ***
Documentation       Verify 5.6.1.4 Create Entity behaviour for registrations
...                 not supporting the operation.
...
...                 5.6.1.4: "For matching redirect Registrations where the
...                 Create Entity operation is not supported, this shall
...                 result in an error of type Conflict if the complete
...                 Create Entity operation failed"; inclusive registrations
...                 are only forwarded "in case the Create Entity operation
...                 is supported". The default operations set (federationOps,
...                 4.20) does not include createEntity.
...
...                 Antares extension TP — official D001 TPs only register
...                 sources that DO support the operation.

Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Teardown       Delete Registration And Stop Mock


*** Variables ***
${entity_payload_filename}      vehicle-simple-attributes.jsonld
${retrieve_ops_file_path}       csourceRegistrations/context-source-registration-vehicle-retrieve-ops.jsonld


*** Test Cases ***
5614_01_01 Redirect Registration Without Create Support Is Conflict
    [Documentation]    5.6.1.4: the whole entity is covered by a redirect
    ...    registration whose source does not support Create Entity → the
    ...    complete create fails with 409 Conflict; the source is never
    ...    contacted and the entity does not exist locally.
    [Tags]    dist-ops    5_6_1    4_20    since_v1.9.1
    Setup Registration With Ops    redirect
    ${response}=    Create Entity    ${entity_payload_filename}    ${entity_id}
    Check Response Status Code    409    ${response.status_code}
    Should Contain    ${response.text}    errors/Conflict
    ${stub_count}=    Get Stub Count    POST    /source/ngsi-ld/v1/entities
    Should Be Equal As Integers    ${stub_count}    0
    ${response}=    Retrieve Entity    ${entity_id}
    Check Response Status Code    404    ${response.status_code}

5614_01_02 Inclusive Registration Without Create Support Is Not Forwarded
    [Documentation]    5.6.1.4: an inclusive registration without Create
    ...    Entity support is simply not forwarded — the local create
    ...    succeeds with 201 and the source is never contacted.
    [Tags]    dist-ops    5_6_1    4_20    since_v1.9.1
    Setup Registration With Ops    inclusive
    ${response}=    Create Entity    ${entity_payload_filename}    ${entity_id}
    Check Response Status Code    201    ${response.status_code}
    ${stub_count}=    Get Stub Count    POST    /source/ngsi-ld/v1/entities
    Should Be Equal As Integers    ${stub_count}    0
    ${response}=    Retrieve Entity    ${entity_id}
    Check Response Status Code    200    ${response.status_code}
    [Teardown]    Run Keywords    Delete Entity    ${entity_id}
    ...    AND    Delete Registration And Stop Mock


5614_01_03 Redirect Registration Without Update Support Is Conflict
    [Documentation]    5.6.2.4: an exclusive/redirect registration matching
    ...    the update but not supporting it → 409 Conflict; the source is
    ...    never contacted.
    [Tags]    dist-ops    5_6_2    4_20    since_v1.9.1
    Setup Registration With Ops    redirect
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${fragment}=    Evaluate    {"speed": {"type": "Property", "value": 9}}
    ${response}=    PATCH
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}/attrs/
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    409    ${response.status_code}
    Should Contain    ${response.text}    errors/Conflict
    ${stub_count}=    Get Stub Count    PATCH    /source/ngsi-ld/v1/entities/${entity_id}/attrs/
    Should Be Equal As Integers    ${stub_count}    0


*** Keywords ***
Setup Registration With Ops
    [Arguments]    ${mode}
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}
    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${retrieve_ops_file_path}
    ...    entity_id=${entity_id}
    ...    mode=${mode}
    ...    endpoint=/source
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}
    Start Context Source Mock Server
    Set Stub Reply    POST    /source/ngsi-ld/v1/entities    201

Delete Registration And Stop Mock
    Delete Context Source Registration    ${registration_id}
    Stop Context Source Mock Server
