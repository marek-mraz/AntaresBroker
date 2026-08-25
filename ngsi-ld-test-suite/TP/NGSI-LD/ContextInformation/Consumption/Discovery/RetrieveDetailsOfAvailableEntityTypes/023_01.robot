*** Settings ***
Documentation       Check that one can retrieve a list with a detailed representation of NGSI-LD entity types

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Entities
Test Teardown       Delete Initial Entities
Test Template       Retrieve Details Of Available Entity Types


*** Variables ***
${first_filename}=      building-simple-attributes.json
${second_filename}=     vehicle-simple-attributes.json


*** Test Cases ***    CONTEXT    EXPECTATION_FILE
023_01_01 WithoutJsonLdContext
    [Tags]    ed-types-details    5_7_6
    ${EMPTY}    types/expectations/entity-type-023-01-01.json
023_01_02 WithJsonLdContext
    [Tags]    ed-types-details    5_7_6
    ${ngsild_test_suite_context}    types/expectations/entity-type-023-01-02.json


*** Keywords ***
Retrieve Details Of Available Entity Types
    [Documentation]    Check that one can retrieve a list with a detailed representation of NGSI-LD entity types
    [Arguments]    ${context}    ${expectation_file}
    ${response}=    Retrieve Entity Types
    ...    context=${context}
    ...    details=true
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing EntityType element    ${expectation_file}    ${response.json()}

Setup Initial Entities
    ${first_entity_id}=    Generate Random Building Entity Id
    ${second_entity_id}=    Generate Random Vehicle Entity Id
    ${create_response1}=    Create Entity Selecting Content Type
    ...    ${first_filename}
    ...    ${first_entity_id}
    ...    ${CONTENT_TYPE_JSON}
    ...    ${ngsild_test_suite_context}
    Check Response Status Code    201    ${create_response1.status_code}
    Set Test Variable    ${first_entity_id}
    ${create_response2}=    Create Entity Selecting Content Type
    ...    ${second_filename}
    ...    ${second_entity_id}
    ...    ${CONTENT_TYPE_JSON}
    ...    ${ngsild_test_suite_context}
    Check Response Status Code    201    ${create_response2.status_code}
    Set Test Variable    ${second_entity_id}

Delete Initial Entities
    Delete Entity    ${first_entity_id}
    Delete Entity    ${second_entity_id}
