*** Settings ***
Documentation       Check that an entity can be retrieved with an inline linked entity with sysAttrs

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Entity And Linked Entity
Test Teardown       Delete Created Entity And Linked Entity


*** Variables ***
${building_id_prefix}=          urn:ngsi-ld:Building:
${city_id_prefix}=              urn:ngsi-ld:City:
${linking_entity_filename}=     building-relationship.jsonld
${linked_entity_filename}=      city-minimal.jsonld


*** Test Cases ***
018_11_01 Retrieve Entity With Inline Linked Entity And SysAttrs
    [Documentation]    Check that an entity can be retrieved with an inline linked entity with sysAttrs
    [Tags]    e-retrieve    5_7_1    4_5_23    since_v1.8.1
    ${response}=    Retrieve Entity
    ...    id=${linking_entity_id}
    ...    join=inline
    ...    joinLevel=1
    ...    options=sysAttrs
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Dictionary Should Contain Key    ${response.json()}[locatedAt][entity]    createdAt


*** Keywords ***
Create Initial Entity And Linked Entity
    ${linking_entity_id}=    Catenate    ${building_id_prefix}018-11
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
