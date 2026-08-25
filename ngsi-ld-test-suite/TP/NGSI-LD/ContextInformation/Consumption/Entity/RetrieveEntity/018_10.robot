*** Settings ***
Documentation       Check that an entity can be retrieved with a linked entity in different join types and representations

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Entity And Linked Entity
Test Teardown       Delete Created Entity And Linked Entity
Test Template       Retrieve Entity With Linked Entity


*** Variables ***
${building_id_prefix}=          urn:ngsi-ld:Building:
${city_id_prefix}=              urn:ngsi-ld:City:
${linking_entity_filename}=     building-relationship.jsonld
${linked_entity_filename}=      city-minimal.jsonld


*** Test Cases ***    JOIN    OPTIONS    EXPECTATION_FILENAME
018_10_01 Inline Normalized
    [Tags]    e-retrieve    5_7_1    4_5_23    since_v1.8.1
    inline    ${EMPTY}    linked-entity-retrieval/building-one-level-inline-018-10.json
018_10_02 Flat Normalized
    [Tags]    e-retrieve    5_7_1    4_5_23    since_v1.8.1
    flat    ${EMPTY}    linked-entity-retrieval/building-one-level-flat-018-10.json
018_10_03 Inline Simplified
    [Tags]    e-retrieve    5_7_1    4_5_23    since_v1.8.1
    inline    keyValues    linked-entity-retrieval/building-one-level-inline-simplified-018-10.json
018_10_04 Flat Simplified
    [Tags]    e-retrieve    5_7_1    4_5_23    since_v1.8.1
    flat    keyValues    linked-entity-retrieval/building-one-level-flat-simplified-018-10.json


*** Keywords ***
Retrieve Entity With Linked Entity
    [Documentation]    Check that an entity can be retrieved with a linked entity in different join types and representations
    [Arguments]    ${join}    ${options}    ${expectation_filename}
    ${response}=    Retrieve Entity
    ...    id=${linking_entity_id}
    ...    join=${join}
    ...    joinLevel=1
    ...    options=${options}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Content
    ...    expectation_filename=${expectation_filename}
    ...    response_body=${response.json()}

Create Initial Entity And Linked Entity
    ${linking_entity_id}=    Catenate    ${building_id_prefix}018-10
    Set Suite Variable    ${linking_entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${linking_entity_filename}
    ...    ${linking_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
    ${linked_entity_id}=    Catenate    ${city_id_prefix}Paris
    Set Suite Variable    ${linked_entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${linked_entity_filename}
    ...    ${linked_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Delete Created Entity And Linked Entity
    Delete Entity    ${linking_entity_id}
    Delete Entity    ${linked_entity_id}
