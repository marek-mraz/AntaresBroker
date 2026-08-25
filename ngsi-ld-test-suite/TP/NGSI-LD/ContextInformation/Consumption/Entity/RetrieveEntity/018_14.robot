*** Settings ***
Documentation       Check that an entity can be retrieved asking for a non-existent linked entity

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Entity
Test Teardown       Delete Created Entity
Test Template       Retrieve Entity With Non Existent Linked Entity


*** Variables ***
${building_id_prefix}=          urn:ngsi-ld:Building:
${linking_entity_filename}=     building-relationship.jsonld


*** Test Cases ***    JOIN    EXPECTATION_FILENAME
018_14_01 Inline
    [Tags]    e-retrieve    5_7_1    4_5_23    since_v1.8.1
    inline    linked-entity-retrieval/building-non-existent-inline-flat-018-14.json
018_14_02 Flat
    [Tags]    e-retrieve    5_7_1    4_5_23    since_v1.8.1
    flat    linked-entity-retrieval/building-non-existent-inline-flat-018-14.json


*** Keywords ***
Retrieve Entity With Non Existent Linked Entity
    [Documentation]    Check that an entity can be retrieved asking for a non-existent linked entity
    [Arguments]    ${join}    ${expectation_filename}
    ${response}=    Retrieve Entity
    ...    id=${linking_entity_id}
    ...    join=${join}
    ...    joinLevel=1
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Content
    ...    expectation_filename=${expectation_filename}
    ...    response_body=${response.json()}

Create Initial Entity
    ${linking_entity_id}=    Catenate    ${building_id_prefix}018-14
    Set Suite Variable    ${linking_entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    filename=${linking_entity_filename}
    ...    entity_id=${linking_entity_id}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Delete Created Entity
    Delete Entity    ${linking_entity_id}
