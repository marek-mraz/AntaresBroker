*** Settings ***
Documentation       Check that one can update entity attributes

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Initialize Test
Test Teardown       Delete Initial Entities
Test Template       Update Attributes


*** Variables ***
${filename}=    vehicle-two-datasetid-attributes.jsonld


*** Test Cases ***    STATUS_CODE    FRAGMENT_FILENAME    EXPECTATION_FILENAME
011_01_01 Check That One Can Update Existing Attributes With No datasetId
    204    vehicle-speed-two-datasetid-01-fragment.jsonld    expectations/vehicle-update-attributes.jsonld
011_01_02 Check That One Can Update Existing Attributes With The datasetId
    204    vehicle-speed-two-datasetid-02-fragment.jsonld    expectations/vehicle-update-datasetid-attributes.jsonld
011_01_03 Check That One Can Update Existing Attributes And Append Non-existing Attributes
    204    vehicle-speed-two-datasetid-03-fragment.jsonld    expectations/vehicle-multi-attributes.jsonld
011_01_04 Check That One Can Change The Type Of An Existing Attribute
    204    vehicle-speed-two-datasetid-04-fragment.jsonld    expectations/vehicle-update-attributes-new-attribute-type.jsonld


*** Keywords ***
Update Attributes
    [Documentation]    Check that one can update entity attributes
    [Tags]    ea-update    5_6_2
    [Arguments]
    ...    ${status_code}
    ...    ${fragment_filename}
    ...    ${expectation_filename}
    ${response}=    Update Entity Attributes
    ...    ${entity_id}
    ...    ${fragment_filename}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    ${status_code}    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${entity_expectation_payload}=    Load Test Sample    entities/${expectation_filename}    ${entity_id}
    ${response1}=    Retrieve Entity
    ...    id=${entity_id}
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ${ignored_attributes}=    Create List    @context
    Check Updated Resource Set To
    ...    updated_resource=${entity_expectation_payload}
    ...    response_body=${response1.json()}
    ...    ignored_keys=${ignored_attributes}

Delete Initial Entities
    Delete Entity    ${entity_id}

Initialize Test
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
